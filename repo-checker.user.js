// ==UserScript==
// @name         Check Repo Archive
// @namespace    http://tampermonkey.net/
// @version      0.2
// @description  Checks if the current GitHub or Hugging Face repository is in the archive
// @author       You
// @match        https://github.com/*/*
// @match        https://huggingface.co/*
// @grant        GM_xmlhttpRequest
// ==/UserScript==

function transformGitHubUrl(url) {
  const parsedUrl = new URL(url);
  const pathname = parsedUrl.pathname;
  const pathSegments = pathname.split("/");
  const org = pathSegments[1];
  const repo = pathSegments[2];

  return `${parsedUrl.protocol}//${parsedUrl.host}/${org}/${repo}`;
}

function transformHuggingfaceUrl(url) {
  const parsedUrl = new URL(url);
  const pathname = parsedUrl.pathname;
  const pathSegments = pathname.split("/");
  const org = pathSegments[1];
  const repo = pathSegments[2];

  return `${org}/${repo}`;
}

function notify(res) {
  if (res.exists) {
    document.body.style.border = "2px solid green";
  } else {
    document.body.style.border = "2px solid red";
  }
}

const cache = new Map();

(function () {
  "use strict";

  async function checkRepoInArchive(url) {
    const hostname = new URL(url).hostname;
    let transformedUrl, apiUrl, requestData;

    if (hostname === "github.com") {
      transformedUrl = transformGitHubUrl(url);
      apiUrl = "http://127.0.0.1:8081/has_git_repo";
      requestData = JSON.stringify({ url: transformedUrl });
    } else if (hostname === "huggingface.co") {
      transformedUrl = transformHuggingfaceUrl(url);
      apiUrl = "http://127.0.0.1:8081/has_huggingface_repo";
      requestData = JSON.stringify({ repo: transformedUrl });
    } else {
      return;
    }

    if (cache.has(transformedUrl)) {
      const cached = cache.get(transformedUrl);

      if (cached !== null) {
        notify(cache.get(transformedUrl));
      }

      return;
    }

    cache.set(transformedUrl, null);

    GM_xmlhttpRequest({
      method: "POST",
      url: apiUrl,
      headers: {
        "Content-Type": "application/json",
      },
      data: requestData,
      onload: (response) => {
        if (response.status === 200) {
          const result = JSON.parse(response.responseText);
          notify(result);
          cache.set(transformedUrl, result);
        } else {
          console.error("Error checking repository:", response.statusText);
        }
      },
      onerror: (error) => {
        document.body.style.border = "2px solid yellow";
        console.log("error: " + JSON.stringify(error));
      },
    });
  }

  checkRepoInArchive(window.location.href);

  const observer = new MutationObserver((mutations) => {
    mutations.forEach((mutation) => {
      if (mutation.type === "childList" || mutation.type === "attributes") {
        if (window.location.href !== document.referrer) {
          checkRepoInArchive(window.location.href);
        }
      }
    });
  });

  observer.observe(document.body, {
    childList: true,
    subtree: true,
    attributes: true,
  });
})();
