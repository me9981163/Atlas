use atlas_parser::python::PythonExtractor;
use atlas_parser::typescript::TypeScriptExtractor;
use atlas_parser::RustExtractor;

#[test]
fn test_python_basic() {
    let mut extractor = PythonExtractor::new();
    let source = r#"
def foo():
    helper()

class Bar:
    def method(self):
        helper()
"#;
    let result = extractor.extract("test.py", source);
    assert!(!result.symbols.is_empty());
    assert!(!result.edges.is_empty());
}

#[test]
fn test_typescript_basic() {
    let mut extractor = TypeScriptExtractor::new();
    let source = r#"
function foo() {
    helper();
}

class Bar {
    method() {
        helper();
    }
}

const f = () => {
    helper();
};

interface I {}
"#;
    let result = extractor.extract("test.ts", source);
    assert!(!result.symbols.is_empty());
    assert!(!result.edges.is_empty());
}

#[test]
fn test_typescript_tsx() {
    let mut extractor = TypeScriptExtractor::new();
    let source = r#"
function App() {
    render();
}
"#;
    let result = extractor.extract("test.tsx", source);
    assert!(!result.symbols.is_empty());
}

#[test]
fn test_python_deep_nesting_no_stack_overflow() {
    let mut extractor = PythonExtractor::new();
    let mut expr = String::from("1");
    for _ in 0..10000 {
        expr.push_str(" + 1");
    }
    let source = format!("def foo():\n    x = {}\n", expr);
    let result = extractor.extract("test.py", &source);
    assert!(!result.symbols.is_empty());
}

#[test]
fn test_typescript_deep_nesting_no_stack_overflow() {
    let mut extractor = TypeScriptExtractor::new();
    let mut expr = String::from("1");
    for _ in 0..10000 {
        expr.push_str(" + 1");
    }
    let source = format!("function foo() {{ let x = {}; }}\n", expr);
    let result = extractor.extract("test.ts", &source);
    assert!(!result.symbols.is_empty());
}

#[test]
fn test_rust_still_works() {
    let mut extractor = RustExtractor::new();
    let source = "fn foo() { helper(); } fn helper() {}";
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 1);
}
