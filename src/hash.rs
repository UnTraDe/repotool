use clap::Parser;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::io::{Seek, Write};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Duration;
use std::{fs, io, path::Path};
use walkdir::WalkDir;

#[derive(Parser, Debug)]
pub struct HashParams {
    #[arg(short, long, default_value = "output.json")]
    output_file_path: PathBuf,

    #[arg(short, long)]
    target_path: PathBuf,

    #[arg(short, long)]
    parallel: Option<usize>,

    #[arg(short, long, default_value_t = 10)]
    sync_interval: usize,

    #[arg(short, long)]
    compare_file: Option<PathBuf>,
}

#[derive(Deserialize, Serialize, Debug)]
struct FileEntry {
    filename: String,
    path: PathBuf,
    sha256: String,
    size: u64,
}

impl FileEntry {
    fn new(path: PathBuf, hash: Vec<u8>, size: u64) -> anyhow::Result<Self> {
        if !path.is_file() {
            return Err(anyhow::anyhow!("{} is not a file", path.display()));
        }

        Ok(Self {
            filename: path
                .file_name()
                .ok_or(anyhow::anyhow!("test"))?
                .to_string_lossy()
                .into_owned(),
            path,
            sha256: base16ct::lower::encode_string(&hash),
            size,
        })
    }
}

fn sha256(path: &Path) -> anyhow::Result<FileEntry> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let _n = io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();

    FileEntry::new(path.to_owned(), hash.to_vec(), file.metadata()?.len())
}

fn is_potentionally_relevant(path: &Path) -> bool {
    if let Ok(Some(t)) = infer::get_from_path(path) {
        match t.matcher_type() {
            infer::MatcherType::Audio
            | infer::MatcherType::Book
            | infer::MatcherType::Doc
            | infer::MatcherType::Font
            | infer::MatcherType::Image
            | infer::MatcherType::Text
            | infer::MatcherType::Video => {}
            _ => return true,
        }
    }

    false
}

pub fn run(args: HashParams) -> anyhow::Result<()> {
    let extensions = [
        "bin",
        "safetensors",
        "pt",
        "pth",
        "onnx",
        "h5",
        "hdf5",
        "ckpt",
        "pb",
        "zip",
        "7z",
        "tar",
        "gz",
        "xz",
        "bz2",
        "rar",
        "lz4",
        "gguf",
    ]
    .map(OsStr::new);

    let mut output_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(args.output_file_path)?;

    let entries = if let Some(compare_file) = args.compare_file {
        serde_json::from_reader::<_, Vec<FileEntry>>(std::fs::File::open(&compare_file)?)?
            .iter()
            .map(|fe| fe.path.clone())
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    let (tx, rx) = std::sync::mpsc::channel();

    let file_writer_thread_handle = std::thread::spawn(move || {
        let mut output_array = vec![];

        while let Ok(entry) = rx.recv() {
            output_array.push(serde_json::to_value(entry).unwrap());

            if output_array.len() % args.sync_interval == 0 {
                output_file.set_len(0).unwrap();
                output_file.rewind().unwrap();
                output_file
                    .write_all(
                        &serde_json::to_vec(&serde_json::Value::Array(output_array.clone()))
                            .unwrap(),
                    )
                    .unwrap();
            }
        }

        output_file.set_len(0).unwrap();
        output_file.rewind().unwrap();
        output_file
            .write_all(
                &serde_json::to_vec(&serde_json::Value::Array(output_array.clone())).unwrap(),
            )
            .unwrap();
    });

    if let Some(threads) = args.parallel {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .unwrap();
    }

    let mut files = vec![];
    let mut potentially_relevant = vec![];
    for p in WalkDir::new(args.target_path).into_iter().filter_map(|e| {
        if let Ok(e) = e {
            if e.metadata().ok()?.is_file() {
                return Some(e.path().to_path_buf());
            }
        }

        None
    }) {
        if entries.contains(&p) {
            log::info!("skipping {}...", p.to_string_lossy());
            continue;
        }

        if let Some(ext) = p.extension() {
            if extensions.contains(&ext.to_os_string().as_ref()) {
                files.push(p);
            } else if is_potentionally_relevant(&p) {
                potentially_relevant.push(p);
            }
        }
    }

    let m = MultiProgress::new();
    let overall_pb = m.add(ProgressBar::new(files.len() as u64));
    overall_pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{percent}%] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
        )
        .unwrap()
        .progress_chars("##-"),
    );

    let errors = files
        .into_par_iter()
        .filter_map(|p| {
            let pb = m.add(ProgressBar::new_spinner());
            pb.set_style(ProgressStyle::with_template("{spinner} {elapsed} {msg}").unwrap());
            let size = fs::metadata(&p)
                .ok()
                .map(|m| m.size() as f64 / 1024.0 / 1024.0);
            pb.set_message(format!("({size:.02?} MB) {}", p.to_string_lossy()));
            pb.enable_steady_tick(Duration::from_millis(100));
            let result = sha256(&p);
            pb.finish_and_clear();
            overall_pb.inc(1);

            match result {
                Ok(file_entry) => {
                    tx.send(file_entry).unwrap();
                    None
                }
                Err(e) => Some((p, e.to_string())),
            }
        })
        .collect::<Vec<_>>();

    overall_pb.finish();
    drop(tx);

    if !errors.is_empty() {
        log::error!("errors({}):", errors.len());
        for (p, e) in errors {
            log::error!("{}: {}", p.to_string_lossy(), e);
        }
    } else {
        log::info!("no errors!");
    }

    if !potentially_relevant.is_empty() {
        log::info!("potentially relevant({}):", potentially_relevant.len());
        for p in potentially_relevant {
            log::info!("{}", p.to_string_lossy());
        }
    }

    file_writer_thread_handle.join().unwrap();

    Ok(())
}
