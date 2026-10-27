use atlas_parser::python::PythonExtractor;
use atlas_parser::typescript::TypeScriptExtractor;
use atlas_parser::RustExtractor;

fn symbol<'a>(result: &'a atlas_core::FileSymbols, name: &str) -> &'a atlas_core::Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

#[test]
fn rust_signatures_and_docs() {
    let mut extractor = RustExtractor::new();
    let source = r#"
/// Adds two numbers.
///
/// Second line.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/** A point in space. */
pub struct Point {
    pub x: i32,
}

/// A color.
#[derive(Debug)]
pub enum Color {
    Red,
}

const MAX: usize = 10;

/// A named type.
pub type Number = i32;
"#;
    let result = extractor.extract("test.rs", source);

    let add = symbol(&result, "add");
    assert_eq!(
        add.signature.as_deref(),
        Some("pub fn add(a: i32, b: i32) -> i32")
    );
    assert_eq!(
        add.doc.as_deref(),
        Some("Adds two numbers.\n\nSecond line.")
    );

    let point = symbol(&result, "Point");
    assert_eq!(point.signature.as_deref(), Some("pub struct Point"));
    assert_eq!(point.doc.as_deref(), Some("A point in space."));

    let color = symbol(&result, "Color");
    assert_eq!(color.signature.as_deref(), Some("pub enum Color"));
    assert_eq!(color.doc.as_deref(), Some("A color."));

    let max = symbol(&result, "MAX");
    assert_eq!(max.signature.as_deref(), Some("const MAX: usize = 10"));
    assert_eq!(max.doc, None);

    let number = symbol(&result, "Number");
    assert_eq!(number.signature.as_deref(), Some("pub type Number = i32"));
    assert_eq!(number.doc.as_deref(), Some("A named type."));
}

#[test]
fn rust_inner_docs_are_not_attached_to_items() {
    let mut extractor = RustExtractor::new();
    let source = r#"
//! Crate-level documentation.

pub fn item() {}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(symbol(&result, "item").doc, None);
}

#[test]
fn python_signatures_and_docstrings() {
    let mut extractor = PythonExtractor::new();
    let source = r#"
# Not the docstring.
def add(a: int, b: int) -> int:
    """Add two numbers.

    Returns the sum.
    """
    return a + b


class Point:
    '''A point.'''

    def __init__(self, x):
        self.x = x


# A helper without a docstring.
def helper():
    pass
"#;
    let result = extractor.extract("test.py", source);

    let add = symbol(&result, "add");
    assert_eq!(
        add.signature.as_deref(),
        Some("def add(a: int, b: int) -> int")
    );
    assert_eq!(
        add.doc.as_deref(),
        Some("Add two numbers.\n\nReturns the sum.")
    );

    let point = symbol(&result, "Point");
    assert_eq!(point.signature.as_deref(), Some("class Point"));
    assert_eq!(point.doc.as_deref(), Some("A point."));

    let helper = symbol(&result, "helper");
    assert_eq!(helper.signature.as_deref(), Some("def helper()"));
    assert_eq!(helper.doc.as_deref(), Some("A helper without a docstring."));
}

#[test]
fn typescript_signatures_and_jsdoc() {
    let mut extractor = TypeScriptExtractor::new();
    let source = r#"
/**
 * Adds two numbers.
 */
export function add(a: number, b: number): number {
    return a + b;
}

/** A point. */
export class Point {
    /** Move the point. */
    move(dx: number): void {}
}

/** Identity. */
export const id = (x: number) => x;

/** A shape. */
export interface Shape {
    area(): number;
}
"#;
    let result = extractor.extract("test.ts", source);

    let add = symbol(&result, "add");
    assert_eq!(
        add.signature.as_deref(),
        Some("export function add(a: number, b: number): number")
    );
    assert_eq!(add.doc.as_deref(), Some("Adds two numbers."));

    let point = symbol(&result, "Point");
    assert_eq!(point.signature.as_deref(), Some("export class Point"));
    assert_eq!(point.doc.as_deref(), Some("A point."));

    let method = symbol(&result, "move");
    assert_eq!(method.signature.as_deref(), Some("move(dx: number): void"));
    assert_eq!(method.doc.as_deref(), Some("Move the point."));

    let id = symbol(&result, "id");
    assert_eq!(
        id.signature.as_deref(),
        Some("export const id = (x: number) =>")
    );
    assert_eq!(id.doc.as_deref(), Some("Identity."));

    let shape = symbol(&result, "Shape");
    assert_eq!(shape.signature.as_deref(), Some("export interface Shape"));
    assert_eq!(shape.doc.as_deref(), Some("A shape."));
}
