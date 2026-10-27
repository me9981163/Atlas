use atlas_core::{EdgeKind, FileSymbols, Import, RawEdge, Symbol, SymbolKind};
use tree_sitter::Parser;

use crate::details::{
    clean_line_doc, clean_python_string, collect_preceding_docs, declaration_signature,
};

pub struct PythonExtractor {
    parser: Parser,
}

impl PythonExtractor {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("Failed to load Python grammar");
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
                "function_definition" => {
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
                            signature: python_signature(node, source),
                            doc: python_doc(node, source),
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
                "class_definition" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::Struct,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: python_signature(node, source),
                            doc: python_doc(node, source),
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
                "function_definition" => {
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
                "class_definition" => {
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
            if node.kind() == "call" {
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
                    let mut cursor = node.walk();
                    let names: Vec<tree_sitter::Node> =
                        node.children_by_field_name("name", &mut cursor).collect();
                    for child in names {
                        match child.kind() {
                            "dotted_name" => imports.push(Import {
                                module: source[child.byte_range()].to_string(),
                                name: None,
                                alias: None,
                                line,
                            }),
                            "aliased_import" => {
                                if let Some(name_node) = child.child_by_field_name("name") {
                                    let alias = child
                                        .child_by_field_name("alias")
                                        .map(|n| source[n.byte_range()].to_string());
                                    imports.push(Import {
                                        module: source[name_node.byte_range()].to_string(),
                                        name: None,
                                        alias,
                                        line,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    continue;
                }
                "import_from_statement" => {
                    let line = node.start_position().row + 1;
                    let module = node
                        .child_by_field_name("module_name")
                        .map(|n| source[n.byte_range()].to_string())
                        .unwrap_or_default();
                    let mut cursor = node.walk();
                    let names: Vec<tree_sitter::Node> =
                        node.children_by_field_name("name", &mut cursor).collect();
                    let mut has_wildcard = false;
                    let mut child_cursor = node.walk();
                    for child in node.children(&mut child_cursor) {
                        if child.kind() == "wildcard_import" {
                            has_wildcard = true;
                        }
                    }
                    if names.is_empty() && has_wildcard {
                        imports.push(Import {
                            module,
                            name: Some("*".to_string()),
                            alias: None,
                            line,
                        });
                    } else {
                        for child in names {
                            match child.kind() {
                                "dotted_name" => imports.push(Import {
                                    module: module.clone(),
                                    name: Some(source[child.byte_range()].to_string()),
                                    alias: None,
                                    line,
                                }),
                                "aliased_import" => {
                                    if let Some(name_node) = child.child_by_field_name("name") {
                                        let alias = child
                                            .child_by_field_name("alias")
                                            .map(|n| source[n.byte_range()].to_string());
                                        imports.push(Import {
                                            module: module.clone(),
                                            name: Some(source[name_node.byte_range()].to_string()),
                                            alias,
                                            line,
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    continue;
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

    fn extract_callee_name(&self, source: &str, function_node: tree_sitter::Node) -> String {
        match function_node.kind() {
            "identifier" => source[function_node.byte_range()].to_string(),
            "attribute" => {
                // Keep the full dotted path (`os.path.join`) so the index can
                // match the receiver against imports. Name matching still uses
                // only the final segment.
                source[function_node.byte_range()].to_string()
            }
            _ => {
                let mut cursor = function_node.walk();
                for child in function_node.children(&mut cursor) {
                    if child.kind() == "identifier" {
                        return source[child.byte_range()].to_string();
                    } else if child.kind() == "attribute" {
                        return self.extract_callee_name(source, child);
                    }
                }
                String::new()
            }
        }
    }
}

impl Default for PythonExtractor {
    fn default() -> Self {
        Self::new()
    }
}

fn python_signature(node: tree_sitter::Node, source: &str) -> Option<String> {
    declaration_signature(node, source, node.child_by_field_name("body"), true)
}

fn python_doc(node: tree_sitter::Node, source: &str) -> Option<String> {
    if let Some(doc) = python_docstring(node, source) {
        return Some(doc);
    }
    let anchor = match node.parent() {
        Some(parent) if parent.kind() == "decorated_definition" => parent,
        _ => node,
    };
    collect_preceding_docs(
        anchor,
        source,
        |text| text.trim_start().starts_with('#'),
        clean_line_doc,
    )
}

fn python_docstring(node: tree_sitter::Node, source: &str) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let first = body.named_child(0)?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let value = first.named_child(0)?;
    if value.kind() != "string" {
        return None;
    }
    clean_python_string(&source[value.byte_range()])
}
