use atlas_core::Import;
use atlas_parser::python::PythonExtractor;
use atlas_parser::typescript::TypeScriptExtractor;
use atlas_parser::RustExtractor;

fn find<'a>(
    imports: &'a [Import],
    module: &str,
    name: Option<&str>,
    alias: Option<&str>,
) -> Option<&'a Import> {
    imports.iter().find(|import| {
        import.module == module
            && import.name.as_deref() == name
            && import.alias.as_deref() == alias
    })
}

#[test]
fn rust_use_paths() {
    let mut extractor = RustExtractor::new();
    let source = r#"
use crate::foo::bar;
use crate::foo::{bar, baz as qux, self, *};
use super::thing;
use self::inner::Thing as T;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
"#;
    let result = extractor.extract("src/lib.rs", source);
    let imports = &result.imports;

    assert!(find(imports, "crate::foo", Some("bar"), None).is_some());
    assert!(find(imports, "crate::foo", Some("baz"), Some("qux")).is_some());
    assert!(find(imports, "crate", Some("foo"), None).is_some());
    assert!(find(imports, "crate::foo", None, None).is_some());
    assert!(find(imports, "super", Some("thing"), None).is_some());
    assert!(find(imports, "self::inner", Some("Thing"), Some("T")).is_some());
    assert!(find(imports, "serde", Some("Deserialize"), None).is_some());
    assert!(find(imports, "serde", Some("Serialize"), None).is_some());
    assert!(find(imports, "std::collections", Some("HashMap"), None).is_some());

    // The first use is on line 2 of the source.
    let first = find(imports, "crate::foo", Some("bar"), None).unwrap();
    assert_eq!(first.line, 2);
}

#[test]
fn rust_nested_use_tree() {
    let mut extractor = RustExtractor::new();
    let source = r#"
use crate::a::{b::{c, d}, e};
"#;
    let result = extractor.extract("src/lib.rs", source);
    assert!(find(&result.imports, "crate::a::b", Some("c"), None).is_some());
    assert!(find(&result.imports, "crate::a::b", Some("d"), None).is_some());
    assert!(find(&result.imports, "crate::a", Some("e"), None).is_some());
}

#[test]
fn python_import_forms() {
    let mut extractor = PythonExtractor::new();
    let source = r#"
import os
import os.path as osp
import a.b, c
from foo.bar import baz
from foo.bar import baz as qux, other
from . import sibling
from ..pkg import thing
from foo import *
"#;
    let result = extractor.extract("app.py", source);
    let imports = &result.imports;

    assert!(find(imports, "os", None, None).is_some());
    assert!(find(imports, "os.path", None, Some("osp")).is_some());
    assert!(find(imports, "a.b", None, None).is_some());
    assert!(find(imports, "c", None, None).is_some());
    assert!(find(imports, "foo.bar", Some("baz"), None).is_some());
    assert!(find(imports, "foo.bar", Some("baz"), Some("qux")).is_some());
    assert!(find(imports, "foo.bar", Some("other"), None).is_some());
    assert!(find(imports, ".", Some("sibling"), None).is_some());
    assert!(find(imports, "..pkg", Some("thing"), None).is_some());
    assert!(find(imports, "foo", Some("*"), None).is_some());
}

#[test]
fn typescript_import_forms() {
    let mut extractor = TypeScriptExtractor::new();
    let source = r#"
import { a, b as c } from "./mod";
import def from "./def";
import * as ns from "./ns";
import "./side";
import type { T } from "./types";
const { x, y: z } = require("./req");
const w = require("./req2");
const v = require("./req3").foo;
"#;
    let result = extractor.extract("src/app.ts", source);
    let imports = &result.imports;

    assert!(find(imports, "./mod", Some("a"), None).is_some());
    assert!(find(imports, "./mod", Some("b"), Some("c")).is_some());
    assert!(find(imports, "./def", None, Some("def")).is_some());
    assert!(find(imports, "./ns", Some("*"), Some("ns")).is_some());
    assert!(find(imports, "./side", None, None).is_some());
    assert!(find(imports, "./types", Some("T"), None).is_some());
    assert!(find(imports, "./req", Some("x"), None).is_some());
    assert!(find(imports, "./req", Some("y"), Some("z")).is_some());
    assert!(find(imports, "./req2", None, Some("w")).is_some());
    assert!(find(imports, "./req3", Some("foo"), Some("v")).is_some());
}

#[test]
fn no_imports_in_plain_code() {
    let mut rust = RustExtractor::new();
    let result = rust.extract("src/lib.rs", "fn main() { helper(); }");
    assert!(result.imports.is_empty());

    let mut python = PythonExtractor::new();
    let result = python.extract("app.py", "def main():\n    helper()\n");
    assert!(result.imports.is_empty());

    let mut typescript = TypeScriptExtractor::new();
    let result = typescript.extract("src/app.ts", "function main() { helper(); }");
    assert!(result.imports.is_empty());
}

#[test]
fn python_keeps_full_receiver_path() {
    let mut extractor = PythonExtractor::new();
    let source = "import os\n\ndef f():\n    os.path.join('a', 'b')\n";
    let result = extractor.extract("app.py", source);
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].to_name, "os.path.join");
}

#[test]
fn typescript_keeps_full_receiver_path() {
    let mut extractor = TypeScriptExtractor::new();
    let source = "import { z } from \"zod\";\nfunction f() { z.string(); }\n";
    let result = extractor.extract("src/app.ts", source);
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].to_name, "z.string");
}
