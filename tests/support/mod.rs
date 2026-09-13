use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Build {
    pub out: PathBuf,
    pub log: String,
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst);
        } else {
            fs::copy(entry.path(), dst).unwrap();
        }
    }
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dreamfish-tests/{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_build(root: &Path) -> Result<Build, String> {
    let out = root.join("out");
    let output = Command::new(env!("CARGO_BIN_EXE_dreamfish"))
        .arg("build")
        .arg("--input")
        .arg(root)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(Build {
            out,
            log: format!("{stdout}\n{stderr}"),
        })
    } else {
        Err(format!("{stdout}\n{stderr}"))
    }
}

pub fn build_fixture(name: &str) -> Build {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let root = tmpdir(&format!("fixture-{name}"));
    copy_dir(&fixture, &root);
    match run_build(&root) {
        Ok(b) => b,
        Err(msg) => panic!("fixture `{name}` failed to build:\n{msg}"),
    }
}

pub fn build_from(name: &str, files: &[(&str, &str)]) -> Result<Build, String> {
    let root = tmpdir(&format!("inline-{name}"));
    for (rel, content) in files {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, content).unwrap();
    }
    run_build(&root)
}

pub fn expect_build_error(name: &str, files: &[(&str, &str)], needle: &str) {
    match build_from(name, files) {
        Ok(_) => panic!("site `{name}` unexpectedly built without error"),
        Err(msg) => assert!(
            msg.contains(needle),
            "\n  expected error containing: `{needle}`\n  actual output:\n{msg}"
        ),
    }
}

impl Build {
    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.out.join(rel))
            .unwrap_or_else(|e| panic!("missing output `{rel}` in {}: {e}", self.out.display()))
    }

    pub fn index(&self) -> String {
        self.read("index.html")
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.out.join(rel).exists()
    }

    pub fn body(&self, rel: &str) -> String {
        let html = self.read(rel);
        extract(&html, "<body>", "</body>").trim().to_string()
    }

    pub fn style(&self, rel: &str) -> String {
        let html = self.read(rel);
        extract(&html, "<style>", "</style>").to_string()
    }

    pub fn script(&self, rel: &str) -> String {
        let html = self.read(rel);
        extract(&html, "<script>", "</script>").to_string()
    }
}

pub fn extract<'a>(html: &'a str, open: &str, close: &str) -> &'a str {
    let start = html.find(open).map(|i| i + open.len()).unwrap_or(0);
    let end = html[start..]
        .find(close)
        .map(|i| start + i)
        .unwrap_or(html.len());
    &html[start..end]
}

pub fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}