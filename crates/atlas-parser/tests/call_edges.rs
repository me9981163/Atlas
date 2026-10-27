use atlas_parser::RustExtractor;

#[test]
fn test_plain_call() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    helper();
}
fn helper() {}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].from, "foo");
    assert_eq!(result.edges[0].to_name, "helper");
}

#[test]
fn test_chained_method_call_uses_field_name() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    builder().line_number(true).build();
}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 3);
    assert_eq!(result.edges[0].to_name, "build");
    assert_eq!(result.edges[1].to_name, "line_number");
    assert_eq!(result.edges[2].to_name, "builder");
}

#[test]
fn test_path_call() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    a::b::foo();
    Type::new();
}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 2);
    assert_eq!(result.edges[0].to_name, "a::b::foo");
    assert_eq!(result.edges[1].to_name, "Type::new");
}

#[test]
fn test_method_call() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    x.method();
}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].to_name, "x.method"); // receiver kept as written
}

#[test]
fn test_generic_call() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    bar::<T>();
}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].to_name, "bar");
}

#[test]
fn test_call_inside_closure() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    let f = || {
        helper();
    };
}
fn helper() {}
"#;
    let result = extractor.extract("test.rs", source);
    // Calls inside closures belong to the enclosing function
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].from, "foo");
    assert_eq!(result.edges[0].to_name, "helper");
}

#[test]
fn test_call_inside_impl_method() {
    let mut extractor = RustExtractor::new();
    let source = r#"
impl Counter {
    fn bump(&mut self) {
        helper();
    }
}
fn helper() {}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 1);
    // Check the from qualified name
    assert!(result.edges[0].from.contains("bump"));
}

#[test]
fn test_function_with_no_calls() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    let x = 5;
}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 0);
}

#[test]
fn test_call_to_undefined_name() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    undefined_function();
}
"#;
    let result = extractor.extract("test.rs", source);
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].to_name, "undefined_function");
}

#[test]
fn test_syntax_error_tolerance() {
    let mut extractor = RustExtractor::new();
    let source = r#"
fn foo() {
    helper(
"#;
    let _result = extractor.extract("test.rs", source);
    // Should not panic, just return what it can
}

#[test]
fn test_deep_nesting_no_stack_overflow() {
    let mut extractor = RustExtractor::new();
    // Generate a very deeply nested expression with 20000 terms
    let mut expr = String::from("1");
    for _ in 0..20000 {
        expr.push_str(" + 1");
    }
    let source = format!(
        r#"
fn foo() {{
    let x = {};
}}
"#,
        expr
    );
    // This must not stack overflow - using iterative traversal
    let result = extractor.extract("test.rs", &source);
    // Just checking it doesn't crash
    assert!(!result.symbols.is_empty());
}
