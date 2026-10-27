use std::path::Path;

#[derive(Debug, Clone)]
pub struct LanguageDef {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub separator: &'static str,
}

pub fn get_language_for_path(path: &Path) -> Option<LanguageDef> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some(LanguageDef {
            name: "rust",
            extensions: &["rs"],
            separator: "::",
        }),
        "py" => Some(LanguageDef {
            name: "python",
            extensions: &["py"],
            separator: ".",
        }),
        "ts" => Some(LanguageDef {
            name: "typescript",
            extensions: &["ts"],
            separator: ".",
        }),
        "tsx" => Some(LanguageDef {
            name: "typescript",
            extensions: &["tsx"],
            separator: ".",
        }),
        "js" | "jsx" | "mjs" => None, // handle only if grammar parses correctly, skip for now
        _ => None,
    }
}
