use atlas_core::{EdgeKind, FileSymbols, Import, RawEdge, Symbol, SymbolKind};
use tree_sitter::Parser;

use crate::details::{clean_block_doc, collect_preceding_docs, declaration_signature};

pub struct TypeScriptExtractor {
    parser: Parser,
}

impl TypeScriptExtractor {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("Failed to load TypeScript grammar");
        Self { parser }
    }

    pub fn extract(&mut self, file: &str, source: &str) -> FileSymbols {
        let tree = match self.parser.parse(source, None) {
            Some(t) => t,
            None => {
                return FileSymbols {
                    file: file.to_string(),
                    symbols: Vec::new(),
                    edges: Vec::new(),
                    imports: Vec::new(),
                };
            }
        };

        let mut symbols = Vec::new();
        self.extract_symbols_iterative(&tree, source, file, &mut symbols);

        let mut edges_vec: Vec<RawEdge> = Vec::new();
        self.extract_edges_iterative(source, &tree, &mut edges_vec);

        let imports = self.extract_imports_iterative(&tree, source);
        FileSymbols {
            file: file.to_string(),
            symbols,
            edges: edges_vec,
            imports,
        }
    }

    fn extract_symbols_iterative(
        &self,
        tree: &tree_sitter::Tree,
        source: &str,
        file: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let mut stack: Vec<(tree_sitter::Node, Vec<String>)> = vec![(tree.root_node(), vec![])];
        while let Some((node, mut scope_stack)) = stack.pop() {
            let kind = node.kind();
            match kind {
                "function_declaration" | "function" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        let qualified = if scope_stack.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}{}", scope_stack.join("."), name)
                        };
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: qualified.clone(),
                            kind: SymbolKind::Function,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: ts_signature(node, source),
                            doc: ts_doc(node, source),
                        });
                        scope_stack.push(format!("{}.", name));
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, scope_stack.clone()));
                        }
                        continue;
                    }
                }
                "method_definition" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        let qualified = if scope_stack.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}{}", scope_stack.join("."), name)
                        };
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: qualified.clone(),
                            kind: SymbolKind::Method,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: ts_signature(node, source),
                            doc: ts_doc(node, source),
                        });
                        scope_stack.push(format!("{}.", name));
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, scope_stack.clone()));
                        }
                        continue;
                    }
                }
                "class_declaration" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::Struct,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: ts_signature(node, source),
                            doc: ts_doc(node, source),
                        });
                        scope_stack.push(format!("{}.", name));
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, scope_stack.clone()));
                        }
                        continue;
                    }
                }
                "interface_declaration" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::Trait,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: ts_signature(node, source),
                            doc: ts_doc(node, source),
                        });
                    }
                }
                "variable_declarator" => {
                    // const f = () => {} style functions
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        // check if value is arrow function
                        if let Some(value) = node.child_by_field_name("value") {
                            if value.kind() == "arrow_function" {
                                symbols.push(Symbol {
                                    name: name.to_string(),
                                    qualified_name: if scope_stack.is_empty() {
                                        name.to_string()
                                    } else {
                                        format!("{}{}", scope_stack.join("."), name)
                                    },
                                    kind: SymbolKind::Function,
                                    file: file.to_string(),
                                    line: node.start_position().row + 1,
                                    column: node.start_position().column,
                                    signature: ts_signature(node, source),
                                    doc: ts_doc(node, source),
                                });
                                scope_stack.push(format!("{}.", name));
                                let mut children: Vec<_> =
                                    node.children(&mut node.walk()).collect();
                                children.reverse();
                                for child in children {
                                    stack.push((child, scope_stack.clone()));
                                }
                                continue;
                            }
                        }
                    }
                }
                _ => {}
            }
            let mut children: Vec<_> = node.children(&mut node.walk()).collect();
            children.reverse();
            for child in children {
                stack.push((child, scope_stack.clone()));
            }
        }
    }

    fn extract_edges_iterative(
        &self,
        source: &str,
        tree: &tree_sitter::Tree,
        edges: &mut Vec<RawEdge>,
    ) {
        let mut function_scopes: Vec<(tree_sitter::Range, String)> = Vec::new();
        let mut stack: Vec<(tree_sitter::Node, Vec<String>)> = vec![(tree.root_node(), vec![])];
        while let Some((node, mut current_scope)) = stack.pop() {
            let kind = node.kind();
            match kind {
                "function_declaration" | "function" | "method_definition" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        let qualified = if current_scope.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}{}", current_scope.join(""), name)
                        };
                        function_scopes.push((node.range(), qualified.clone()));
                        current_scope.push(format!("{}.", name));
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, current_scope.clone()));
                        }
                        continue;
                    }
                }
                "class_declaration" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        current_scope.push(format!("{}.", name));
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, current_scope.clone()));
                        }
                        continue;
                    }
                }
                "variable_declarator" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        if let Some(value) = node.child_by_field_name("value") {
                            if value.kind() == "arrow_function" {
                                let name = &source[name_node.byte_range()];
                                let qualified = if current_scope.is_empty() {
                                    name.to_string()
                                } else {
                                    format!("{}{}", current_scope.join(""), name)
                                };
                                function_scopes.push((node.range(), qualified.clone()));
                                current_scope.push(format!("{}.", name));
                                let mut children: Vec<_> =
                                    node.children(&mut node.walk()).collect();
                                children.reverse();
                                for child in children {
                                    stack.push((child, current_scope.clone()));
                                }
                                continue;
                            }
                        }
                    }
                }
                _ => {}
            }
            let mut children: Vec<_> = node.children(&mut node.walk()).collect();
            children.reverse();
            for child in children {
                stack.push((child, current_scope.clone()));
            }
        }

        let mut calls_to_process: Vec<tree_sitter::Node> = Vec::new();
        let mut work_stack = vec![tree.root_node()];
        while let Some(node) = work_stack.pop() {
            if node.kind() == "call_expression" || node.kind() == "new_expression" {
                calls_to_process.push(node);
            }
            let mut c = node.walk();
            for child in node.children(&mut c) {
                work_stack.push(child);
            }
        }
        calls_to_process.sort_by_key(|n| (n.start_position().row, n.start_position().column));

        for call_node in calls_to_process {
            let call_start = call_node.start_byte();
            let mut best: Option<(String, usize)> = None;
            for (range, qname) in &function_scopes {
                if call_start >= range.start_byte && call_start < range.end_byte {
                    let size = range.end_byte - range.start_byte;
                    if let Some((_, best_size)) = best {
                        if size < best_size {
                            best = Some((qname.clone(), size));
                        }
                    } else {
                        best = Some((qname.clone(), size));
                    }
                }
            }
            let from = match best {
                Some((qname, _)) => qname,
                None => continue,
            };

            if let Some(function_node) = call_node.child_by_field_name("function") {
                let to_name = self.extract_callee_name(source, function_node);
                if to_name.is_empty() {
                    continue;
                }
                edges.push(RawEdge {
                    from,
                    to_name,
                    kind: EdgeKind::Calls,
                    line: call_node.start_position().row + 1,
                });
            } else if call_node.kind() == "new_expression" {
                if let Some(constructor) = call_node.child_by_field_name("constructor") {
                    let to_name = self.extract_callee_name(source, constructor);
                    if !to_name.is_empty() {
                        edges.push(RawEdge {
                            from,
                            to_name,
                            kind: EdgeKind::Calls,
                            line: call_node.start_position().row + 1,
                        });
                    }
                }
            }
        }
    }

    fn extract_imports_iterative(&self, tree: &tree_sitter::Tree, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "import_statement" => {
                    let line = node.start_position().row + 1;
                    let module = match node
                        .child_by_field_name("source")
                        .and_then(|n| string_content(source, n))
                    {
                        Some(m) => m,
                        None => continue,
                    };
                    let mut cursor = node.walk();
                    let clause = node
                        .children(&mut cursor)
                        .find(|c| c.kind() == "import_clause");
                    if let Some(clause) = clause {
                        self.extract_import_clause(clause, source, &module, line, &mut imports);
                    } else {
                        // Side-effect import: `import "./styles.css";`
                        imports.push(Import {
                            module,
                            name: None,
                            alias: None,
                            line,
                        });
                    }
                    continue;
                }
                "variable_declarator" => {
                    if let Some(value) = node.child_by_field_name("value") {
                        self.extract_require_imports(node, value, source, &mut imports);
                    }
                }
                _ => {}
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        imports.sort_by(|a, b| {
            (a.line, &a.module, &a.name, &a.alias).cmp(&(b.line, &b.module, &b.name, &b.alias))
        });
        imports
    }

    fn extract_import_clause(
        &self,
        clause: tree_sitter::Node,
        source: &str,
        module: &str,
        line: usize,
        imports: &mut Vec<Import>,
    ) {
        let mut cursor = clause.walk();
        for child in clause.named_children(&mut cursor) {
            match child.kind() {
                "named_imports" => {
                    let mut spec_cursor = child.walk();
                    for spec in child.named_children(&mut spec_cursor) {
                        if spec.kind() != "import_specifier" {
                            continue;
                        }
                        let name = spec
                            .child_by_field_name("name")
                            .map(|n| source[n.byte_range()].to_string());
                        let alias = spec
                            .child_by_field_name("alias")
                            .map(|n| source[n.byte_range()].to_string());
                        if let Some(name) = name {
                            imports.push(Import {
                                module: module.to_string(),
                                name: Some(name),
                                alias,
                                line,
                            });
                        }
                    }
                }
                "namespace_import" => {
                    let alias = child
                        .named_child(0)
                        .map(|n| source[n.byte_range()].to_string());
                    imports.push(Import {
                        module: module.to_string(),
                        name: Some("*".to_string()),
                        alias,
                        line,
                    });
                }
                "identifier" => {
                    // Default import: `import foo from "./foo";`
                    imports.push(Import {
                        module: module.to_string(),
                        name: None,
                        alias: Some(source[child.byte_range()].to_string()),
                        line,
                    });
                }
                _ => {}
            }
        }
    }

    fn extract_require_imports(
        &self,
        declarator: tree_sitter::Node,
        value: tree_sitter::Node,
        source: &str,
        imports: &mut Vec<Import>,
    ) {
        let line = declarator.start_position().row + 1;
        match value.kind() {
            "call_expression" => {
                if !is_require_call(value, source) {
                    return;
                }
                let module = match require_argument(value, source) {
                    Some(m) => m,
                    None => return,
                };
                let name_node = match declarator.child_by_field_name("name") {
                    Some(n) => n,
                    None => return,
                };
                match name_node.kind() {
                    "identifier" => imports.push(Import {
                        module,
                        name: None,
                        alias: Some(source[name_node.byte_range()].to_string()),
                        line,
                    }),
                    "object_pattern" => {
                        let mut cursor = name_node.walk();
                        for prop in name_node.named_children(&mut cursor) {
                            match prop.kind() {
                                "shorthand_property_identifier_pattern" => {
                                    imports.push(Import {
                                        module: module.clone(),
                                        name: Some(source[prop.byte_range()].to_string()),
                                        alias: None,
                                        line,
                                    });
                                }
                                "pair_pattern" => {
                                    let key = prop
                                        .child_by_field_name("key")
                                        .map(|n| source[n.byte_range()].to_string());
                                    let val = prop
                                        .child_by_field_name("value")
                                        .map(|n| source[n.byte_range()].to_string());
                                    if let Some(key) = key {
                                        let alias = val.filter(|v| v != &key);
                                        imports.push(Import {
                                            module: module.clone(),
                                            name: Some(key),
                                            alias,
                                            line,
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            "member_expression" => {
                // `const v = require("./mod").foo;`
                let object = match value.child_by_field_name("object") {
                    Some(o) => o,
                    None => return,
                };
                if object.kind() != "call_expression" || !is_require_call(object, source) {
                    return;
                }
                let module = match require_argument(object, source) {
                    Some(m) => m,
                    None => return,
                };
                let name = match value
                    .child_by_field_name("property")
                    .map(|n| source[n.byte_range()].to_string())
                {
                    Some(n) => n,
                    None => return,
                };
                if let Some(name_node) = declarator.child_by_field_name("name") {
                    if name_node.kind() == "identifier" {
                        imports.push(Import {
                            module,
                            name: Some(name),
                            alias: Some(source[name_node.byte_range()].to_string()),
                            line,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn extract_callee_name(&self, source: &str, function_node: tree_sitter::Node) -> String {
        match function_node.kind() {
            "identifier" => source[function_node.byte_range()].to_string(),
            "member_expression" => {
                // Keep the full member path (`ns.foo`) so the index can match
                // the receiver against imports. Name matching uses the last
                // segment only.
                source[function_node.byte_range()].to_string()
            }
            _ => {
                let mut cursor = function_node.walk();
                for child in function_node.children(&mut cursor) {
                    if child.kind() == "identifier" {
                        return source[child.byte_range()].to_string();
                    } else if child.kind() == "member_expression" {
                        return self.extract_callee_name(source, child);
                    }
                }
                String::new()
            }
        }
    }
}

impl Default for TypeScriptExtractor {
    fn default() -> Self {
        Self::new()
    }
}

fn string_content(source: &str, node: tree_sitter::Node) -> Option<String> {
    let text = &source[node.byte_range()];
    let trimmed = text.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_require_call(call: tree_sitter::Node, source: &str) -> bool {
    call.child_by_field_name("function")
        .map(|f| f.kind() == "identifier" && &source[f.byte_range()] == "require")
        .unwrap_or(false)
}

fn require_argument(call: tree_sitter::Node, source: &str) -> Option<String> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let first = args.named_children(&mut cursor).next()?;
    if first.kind() != "string" {
        return None;
    }
    string_content(source, first)
}

/// Include `export`, `const` and `let` in signatures/docs for declarations
/// that are wrapped by those nodes.
fn ts_outer_declaration(node: tree_sitter::Node) -> tree_sitter::Node {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "export_statement" | "lexical_declaration" | "variable_declaration" => {
                current = parent;
            }
            _ => break,
        }
    }
    current
}

fn ts_signature(node: tree_sitter::Node, source: &str) -> Option<String> {
    let body = if node.kind() == "variable_declarator" {
        node.child_by_field_name("value")
            .and_then(|value| value.child_by_field_name("body"))
    } else {
        node.child_by_field_name("body")
    };
    declaration_signature(ts_outer_declaration(node), source, body, false)
}

fn ts_doc(node: tree_sitter::Node, source: &str) -> Option<String> {
    collect_preceding_docs(
        ts_outer_declaration(node),
        source,
        |text| text.trim_start().starts_with("/**"),
        clean_block_doc,
    )
}
