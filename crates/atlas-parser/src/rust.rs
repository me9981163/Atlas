use atlas_core::{EdgeKind, FileSymbols, Import, RawEdge, Symbol, SymbolKind};
use tree_sitter::Parser;

use crate::details::{
    clean_block_doc, clean_line_doc, collect_preceding_docs, declaration_signature,
};

pub struct RustExtractor {
    parser: Parser,
}

impl RustExtractor {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("Failed to load Rust grammar");
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
        self.extract_edges_iterative(source, &tree, file, &mut edges_vec);

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
        // Iterative traversal
        let mut stack: Vec<(tree_sitter::Node, Vec<String>)> = vec![(tree.root_node(), vec![])];
        while let Some((node, mut scope_stack)) = stack.pop() {
            let kind = node.kind();

            match kind {
                "function_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        let qualified = if scope_stack.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}::{}", scope_stack.join("::"), name)
                        };
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: qualified.clone(),
                            kind: SymbolKind::Function,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: rust_signature(node, source),
                            doc: rust_doc(node, source),
                        });
                        scope_stack.push(name.to_string());
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, scope_stack.clone()));
                        }
                        continue;
                    }
                }
                "impl_item" => {
                    if let Some(type_node) = node.child_by_field_name("type") {
                        let type_name = &source[type_node.byte_range()];
                        scope_stack.push(format!("impl {}", type_name));
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, scope_stack.clone()));
                        }
                        continue;
                    }
                }
                "struct_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::Struct,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: rust_signature(node, source),
                            doc: rust_doc(node, source),
                        });
                    }
                }
                "enum_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::Enum,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: rust_signature(node, source),
                            doc: rust_doc(node, source),
                        });
                    }
                }
                "trait_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::Trait,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: rust_signature(node, source),
                            doc: rust_doc(node, source),
                        });
                    }
                }
                "mod_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        let qualified = if scope_stack.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}::{}", scope_stack.join("::"), name)
                        };
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: qualified.clone(),
                            kind: SymbolKind::Module,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: rust_signature(node, source),
                            doc: rust_doc(node, source),
                        });
                        scope_stack.push(name.to_string());
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, scope_stack.clone()));
                        }
                        continue;
                    }
                }
                "const_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::Const,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: rust_signature(node, source),
                            doc: rust_doc(node, source),
                        });
                    }
                }
                "static_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::Static,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: rust_signature(node, source),
                            doc: rust_doc(node, source),
                        });
                    }
                }
                "type_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        symbols.push(Symbol {
                            name: name.to_string(),
                            qualified_name: name.to_string(),
                            kind: SymbolKind::TypeAlias,
                            file: file.to_string(),
                            line: node.start_position().row + 1,
                            column: node.start_position().column,
                            signature: rust_signature(node, source),
                            doc: rust_doc(node, source),
                        });
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
        &mut self,
        source: &str,
        tree: &tree_sitter::Tree,
        _file: &str,
        edges: &mut Vec<RawEdge>,
    ) {
        // Collect function scopes iteratively
        let mut function_scopes: Vec<(tree_sitter::Range, String)> = Vec::new();
        let mut stack: Vec<(tree_sitter::Node, Vec<String>)> = vec![(tree.root_node(), vec![])];
        while let Some((node, mut current_scope)) = stack.pop() {
            let kind = node.kind();
            match kind {
                "function_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        let qualified = if current_scope.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}::{}", current_scope.join("::"), name)
                        };
                        function_scopes.push((node.range(), qualified.clone()));
                        current_scope.push(name.to_string());
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, current_scope.clone()));
                        }
                        continue;
                    }
                }
                "impl_item" => {
                    if let Some(type_node) = node.child_by_field_name("type") {
                        let type_name = &source[type_node.byte_range()];
                        current_scope.push(format!("impl {}", type_name));
                        let mut children: Vec<_> = node.children(&mut node.walk()).collect();
                        children.reverse();
                        for child in children {
                            stack.push((child, current_scope.clone()));
                        }
                        continue;
                    }
                }
                "mod_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = &source[name_node.byte_range()];
                        current_scope.push(name.to_string());
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

        // Find all call expressions iteratively - walk expression trees iteratively
        let mut calls_to_process: Vec<tree_sitter::Node> = Vec::new();
        let mut work_stack = vec![tree.root_node()];
        while let Some(node) = work_stack.pop() {
            if node.kind() == "call_expression" {
                calls_to_process.push(node);
            }
            let mut c = node.walk();
            for child in node.children(&mut c) {
                work_stack.push(child);
            }
        }
        // Sort by source order for deterministic output
        calls_to_process.sort_by_key(|n| (n.start_position().row, n.start_position().column));

        for call_node in calls_to_process {
            let call_start = call_node.start_byte();
            // Find the innermost containing function
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
                None => continue, // Calls outside any function ignored
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
            if node.kind() == "use_declaration" {
                if let Some(argument) = node.child_by_field_name("argument") {
                    let line = node.start_position().row + 1;
                    self.flatten_use_tree(argument, source, "", line, &mut imports);
                }
                // A use declaration cannot contain another use declaration.
                continue;
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

    /// Flatten a Rust use tree into individual imports. The traversal is
    /// iterative because use trees can be nested arbitrarily deep.
    fn flatten_use_tree(
        &self,
        root: tree_sitter::Node,
        source: &str,
        prefix: &str,
        line: usize,
        imports: &mut Vec<Import>,
    ) {
        let mut stack: Vec<(tree_sitter::Node, String)> = vec![(root, prefix.to_string())];
        while let Some((node, prefix)) = stack.pop() {
            match node.kind() {
                "use_list" => {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.is_named() {
                            stack.push((child, prefix.clone()));
                        }
                    }
                }
                "scoped_use_list" => {
                    let path = node
                        .child_by_field_name("path")
                        .map(|n| source[n.byte_range()].to_string())
                        .unwrap_or_default();
                    let next_prefix = join_path(&prefix, &path);
                    if let Some(list) = node.child_by_field_name("list") {
                        stack.push((list, next_prefix));
                    }
                }
                "use_as_clause" => {
                    let alias = node
                        .child_by_field_name("alias")
                        .map(|n| source[n.byte_range()].to_string());
                    if let Some(path) = node.child_by_field_name("path") {
                        if path.kind() == "self" {
                            // `use foo::{self as f}` imports the module itself.
                            if let Some((parent, last)) = split_last_segment(&prefix) {
                                imports.push(Import {
                                    module: parent.to_string(),
                                    name: Some(last.to_string()),
                                    alias,
                                    line,
                                });
                            }
                        } else {
                            push_use_item(path, source, &prefix, alias, line, imports);
                        }
                    }
                }
                "use_wildcard" => {
                    // `use foo::*` carries a path; `use foo::{*}` does not and
                    // uses the surrounding prefix.
                    let module = node
                        .child_by_field_name("path")
                        .map(|n| join_path(&prefix, &source[n.byte_range()]))
                        .unwrap_or_else(|| prefix.clone());
                    imports.push(Import {
                        module,
                        name: None,
                        alias: None,
                        line,
                    });
                }
                "self" => {
                    // `use foo::{self}` imports the module itself.
                    if let Some((parent, last)) = split_last_segment(&prefix) {
                        imports.push(Import {
                            module: parent.to_string(),
                            name: Some(last.to_string()),
                            alias: None,
                            line,
                        });
                    }
                }
                _ => push_use_item(node, source, &prefix, None, line, imports),
            }
        }
    }

    fn extract_callee_name(&self, source: &str, function_node: tree_sitter::Node) -> String {
        match function_node.kind() {
            "identifier" => source[function_node.byte_range()].to_string(),
            "field_expression" => {
                // Keep a simple receiver path (`x.method`) so the index does
                // not mistake a method call for an imported free function.
                // For complex receivers such as `builder().method()` the full
                // expression is noisy, so only the field name is kept. Name
                // matching uses the final segment either way.
                let field = function_node
                    .child_by_field_name("field")
                    .map(|n| source[n.byte_range()].to_string())
                    .unwrap_or_default();
                let receiver = function_node
                    .child_by_field_name("value")
                    .and_then(|value| {
                        matches!(
                            value.kind(),
                            "identifier"
                                | "field_expression"
                                | "scoped_identifier"
                                | "self"
                                | "super"
                        )
                        .then(|| source[value.byte_range()].trim().to_string())
                    });
                match receiver {
                    Some(receiver) if !field.is_empty() => format!("{}.{}", receiver, field),
                    _ => field,
                }
            }
            "scoped_identifier" => {
                // a::b::foo() and Type::new() -> keep the path as written
                source[function_node.byte_range()].to_string()
            }
            "generic_function" => {
                // Generic calls such as foo::<T>() -> unwrap to the plain name
                if let Some(function) = function_node.child_by_field_name("function") {
                    self.extract_callee_name(source, function)
                } else {
                    let mut cursor = function_node.walk();
                    for child in function_node.children(&mut cursor) {
                        if child.kind() == "identifier" || child.kind() == "scoped_identifier" {
                            return source[child.byte_range()].to_string();
                        } else if child.kind() == "field_expression" {
                            let result = self.extract_callee_name(source, child);
                            if !result.is_empty() {
                                return result;
                            }
                        }
                    }
                    String::new()
                }
            }
            "macro_invocation" => {
                // Macro invocations are ignored in this milestone
                String::new()
            }
            _ => {
                let mut cursor = function_node.walk();
                for child in function_node.children(&mut cursor) {
                    if child.kind() == "identifier" || child.kind() == "scoped_identifier" {
                        return source[child.byte_range()].to_string();
                    } else if child.kind() == "field_expression" {
                        let result = self.extract_callee_name(source, child);
                        if !result.is_empty() {
                            return result;
                        }
                    } else if child.kind() == "generic_function" {
                        return self.extract_callee_name(source, child);
                    }
                }
                String::new()
            }
        }
    }
}

impl Default for RustExtractor {
    fn default() -> Self {
        Self::new()
    }
}

fn join_path(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_string()
    } else if path.is_empty() {
        prefix.to_string()
    } else {
        format!("{}::{}", prefix, path)
    }
}

/// Split `a::b::c` into (`a::b`, `c`). A bare name has an empty module.
fn split_last_segment(path: &str) -> Option<(&str, &str)> {
    let path = path.trim_start_matches("::");
    if path.is_empty() {
        return None;
    }
    match path.rsplit_once("::") {
        Some((parent, last)) => Some((parent, last)),
        None => Some(("", path)),
    }
}

fn push_use_item(
    node: tree_sitter::Node,
    source: &str,
    prefix: &str,
    alias: Option<String>,
    line: usize,
    imports: &mut Vec<Import>,
) {
    let text = &source[node.byte_range()];
    let full = join_path(prefix, text);
    if let Some((module, name)) = split_last_segment(&full) {
        imports.push(Import {
            module: module.to_string(),
            name: Some(name.to_string()),
            alias,
            line,
        });
    }
}

fn rust_signature(node: tree_sitter::Node, source: &str) -> Option<String> {
    declaration_signature(node, source, node.child_by_field_name("body"), false)
}

fn rust_doc(node: tree_sitter::Node, source: &str) -> Option<String> {
    collect_preceding_docs(
        node,
        source,
        |text| {
            let text = text.trim_start();
            text.starts_with("///") || text.starts_with("/**")
        },
        |text| {
            if text.trim_start().starts_with("//") {
                clean_line_doc(text)
            } else {
                clean_block_doc(text)
            }
        },
    )
}
