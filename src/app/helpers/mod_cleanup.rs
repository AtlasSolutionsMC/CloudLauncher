use super::super::*;

#[derive(Default)]
pub(in crate::app) struct ModCleanupSummary {
    pub(crate) removed: Vec<String>,
    pub(crate) failed: Vec<(String, String)>,
}

pub(in crate::app) fn remove_undesirable_mod_files(
    mods_directory: &std::path::Path,
) -> std::io::Result<ModCleanupSummary> {
    static PATTERNS: std::sync::OnceLock<regex::RegexSet> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        regex::RegexSet::new([r"tl.?skin.?cape.*\.jar", r"kl.?master.*\.jar"])
            .expect("undesirable mod filename patterns are valid")
    });
    let mut summary = ModCleanupSummary::default();
    if !mods_directory.exists() {
        return Ok(summary);
    }

    for entry in fs::read_dir(mods_directory)? {
        let entry = entry?;
        if !entry.metadata()?.is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_lowercase();
        if !patterns.is_match(&filename) {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => summary.removed.push(filename),
            Err(error) => summary.failed.push((filename, error.to_string())),
        }
    }
    Ok(summary)
}
