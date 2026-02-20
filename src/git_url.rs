use std::collections::HashSet;

use crate::clone;

/// Extract repository name from a URL
/// Handles formats like:
/// - https://github.com/owner/repo.git
/// - https://github.com/owner/repo
/// - git@github.com:owner/repo.git
pub fn extract_repo_name(url: &str, preserve_dot_git: bool) -> Option<String> {
    // Handle SSH format: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some(path) = rest.split(':').nth(1) {
            let mut name = path.rsplit('/').next()?.to_string();

            if !preserve_dot_git {
                name = name.trim_end_matches(".git").to_string()
            }

            if !name.is_empty() {
                return Some(name);
            }
        }
    }

    // Handle HTTPS format: https://github.com/owner/repo.git
    let path = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;

    let mut name = path.trim_end_matches('/').rsplit('/').next()?.to_string();

    if !preserve_dot_git {
        name = name.trim_end_matches(".git").to_string()
    }

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub fn is_in_compare_list(url: &str, compare: &HashSet<String>) -> bool {
    if compare.contains(url) {
        return true;
    }

    if let Some(url) = url.strip_suffix(".git") {
        if compare.contains(url) {
            return true;
        }
    } else if compare.contains(&format!("{}.git", url)) {
        return true;
    }

    // TODO(tomer) compare across different schemes as well?

    false
}

/// Filter entries that are not in the compare list
pub fn filter_by_compare_list(entries: Vec<clone::Entry>, compare: &HashSet<String>) -> Vec<clone::Entry> {
    entries
        .into_iter()
        .filter(|e| !is_in_compare_list(&e.clone_url, compare))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_repo_name_https_with_git() {
        assert_eq!(
            extract_repo_name("https://github.com/owner/repo.git", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https_without_git() {
        assert_eq!(
            extract_repo_name("https://github.com/owner/repo", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https_trailing_slash() {
        assert_eq!(
            extract_repo_name("https://github.com/owner/repo/", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_ssh_with_git() {
        assert_eq!(
            extract_repo_name("git@github.com:owner/repo.git", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_ssh_without_git() {
        assert_eq!(
            extract_repo_name("git@github.com:owner/repo", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_http() {
        assert_eq!(
            extract_repo_name("http://github.com/owner/repo.git", false),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https_with_git_preserve() {
        assert_eq!(
            extract_repo_name("https://github.com/owner/repo.git", true),
            Some("repo.git".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_https_without_git_preserve() {
        assert_eq!(
            extract_repo_name("https://github.com/owner/repo", true),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_invalid() {
        assert_eq!(extract_repo_name("not-a-url", false), None);
    }
}
