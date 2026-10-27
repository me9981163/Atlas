use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    Const,
    Static,
    TypeAlias,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: usize,
    pub column: usize,
    /// Declaration header as written, without the body. Optional because
    /// not every grammar node has a useful signature.
    #[serde(default)]
    pub signature: Option<String>,
    /// Documentation comment or docstring attached to the declaration.
    #[serde(default)]
    pub doc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Calls,
}

/// A call as written in source. The parser does not know symbol ids;
/// resolving `to_name` to a real symbol happens later, in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawEdge {
    pub from: String,    // qualified name of the enclosing function/method
    pub to_name: String, // callee as written
    pub kind: EdgeKind,
    pub line: usize, // 1-based line of the call
}

/// An import as written in source, e.g. `use crate::foo::bar;`,
/// `from foo.bar import baz`, or `import { baz } from "./bar"`.
///
/// `module` is the module specifier, `name` the imported item (if any), and
/// `alias` the local name when it differs from `name`. The index uses these
/// to resolve a callee through the caller's imports before falling back to
/// name matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    pub module: String,
    pub name: Option<String>,
    pub alias: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSymbols {
    pub file: String,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<RawEdge>,
    #[serde(default)]
    pub imports: Vec<Import>,
}

#[derive(Debug, thiserror::Error)]
pub enum AtlasError {
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_deserializes_without_signature_or_doc() {
        let json = r#"{
            "name": "parse",
            "qualified_name": "parser::parse",
            "kind": "Function",
            "file": "src/parser.rs",
            "line": 3,
            "column": 0
        }"#;
        let symbol: Symbol = serde_json::from_str(json).unwrap();
        assert_eq!(symbol.name, "parse");
        assert_eq!(symbol.signature, None);
        assert_eq!(symbol.doc, None);
    }
}
