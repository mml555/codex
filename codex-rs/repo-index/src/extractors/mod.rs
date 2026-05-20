//! Optional deep language extractors (Milestone 6). Disabled by default.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::signals::RepoSignals;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractorOutput {
    pub language: String,
    pub confidence: f64,
    pub symbols: Vec<String>,
    pub evidence: Vec<String>,
}

pub trait LanguageExtractor: Send + Sync {
    fn id(&self) -> &'static str;
    fn detect(&self, root: &Path) -> bool;
    fn extract(&self, root: &Path, files: &[PathBuf]) -> ExtractorOutput;
}

/// Rust extractor stub — returns low-confidence hints until syn/tree-sitter wiring lands.
#[derive(Debug, Default)]
pub struct RustExtractor;

impl LanguageExtractor for RustExtractor {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn detect(&self, root: &Path) -> bool {
        root.join("Cargo.toml").is_file()
    }

    fn extract(&self, root: &Path, files: &[PathBuf]) -> ExtractorOutput {
        let mut symbols = Vec::new();
        for file in files {
            if file.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(root.join(file)) {
                for line in content.lines().take(300) {
                    let trimmed = line.trim();
                    if trimmed.starts_with("pub fn ") {
                        symbols.push(trimmed.to_string());
                    }
                }
            }
        }
        symbols.sort();
        symbols.dedup();
        ExtractorOutput {
            language: "rust".to_string(),
            confidence: if symbols.is_empty() { 0.4 } else { 0.65 },
            symbols,
            evidence: vec!["extractor:rust_stub".to_string()],
        }
    }
}

#[derive(Debug, Default)]
pub struct TypeScriptExtractor;

impl LanguageExtractor for TypeScriptExtractor {
    fn id(&self) -> &'static str {
        "typescript"
    }

    fn detect(&self, root: &Path) -> bool {
        root.join("package.json").is_file()
    }

    fn extract(&self, _root: &Path, files: &[PathBuf]) -> ExtractorOutput {
        let count = files
            .iter()
            .filter(|p| {
                matches!(
                    p.extension().and_then(|s| s.to_str()),
                    Some("ts") | Some("tsx") | Some("js") | Some("jsx")
                )
            })
            .count();
        ExtractorOutput {
            language: "typescript".to_string(),
            confidence: if count == 0 { 0.35 } else { 0.6 },
            symbols: Vec::new(),
            evidence: vec![format!("extractor:ts_files:{count}")],
        }
    }
}

#[derive(Debug, Default)]
pub struct PythonExtractor;

impl LanguageExtractor for PythonExtractor {
    fn id(&self) -> &'static str {
        "python"
    }

    fn detect(&self, root: &Path) -> bool {
        root.join("pyproject.toml").is_file() || root.join("setup.py").is_file()
    }

    fn extract(&self, _root: &Path, files: &[PathBuf]) -> ExtractorOutput {
        let count = files
            .iter()
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("py"))
            .count();
        ExtractorOutput {
            language: "python".to_string(),
            confidence: if count == 0 { 0.35 } else { 0.6 },
            symbols: Vec::new(),
            evidence: vec![format!("extractor:py_files:{count}")],
        }
    }
}

pub fn apply_deep_extractors(
    root: &Path,
    files: &[PathBuf],
    signals: &mut RepoSignals,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    let extractors: Vec<Box<dyn LanguageExtractor>> = vec![
        Box::new(RustExtractor),
        Box::new(TypeScriptExtractor),
        Box::new(PythonExtractor),
    ];
    for extractor in extractors {
        if !extractor.detect(root) {
            continue;
        }
        let output = extractor.extract(root, files);
        signals.confidence = signals.confidence.max(output.confidence * 0.5);
        for ev in output.evidence {
            signals.evidence.push(ev);
        }
    }
}
