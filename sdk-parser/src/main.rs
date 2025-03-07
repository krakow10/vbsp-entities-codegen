use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::read_to_string;
use std::io::{stdout, Write};
use std::path::PathBuf;
use clap::{Parser, ValueEnum};
use serde::Serialize;
use tree_sitter::{Language, Node, Query, QueryCursor, StreamingIterator};
use walkdir::WalkDir;

#[derive(Parser)]
struct Args {
    /// Path of the source sdk
    sdk_path: PathBuf,
    mode: ParseMode
}

#[derive(ValueEnum, Copy, Clone)]
enum ParseMode {
    Inherits,
    Types,
    EntityClasses,
}

fn main() {
    let args = Args::parse();

    println!("[");

    let mut stdout = stdout().lock();
    let dir = WalkDir::new(&args.sdk_path);
    let mut first = true;

    for file in dir {
        let file = file.unwrap();
        if file.file_type().is_file() {
            let path = file.path();
            if path.extension() == Some(OsStr::new("h")) || path.extension() == Some(OsStr::new("cpp")) {
                match read_to_string(path) {
                    Ok(code) => {
                        let (types, inherits) = parse_file(&code);
                        match args.mode {
                            ParseMode::Types => {
                                print_json_items(&mut stdout, types, &mut first);
                            }
                            ParseMode::Inherits => {
                                print_json_items(&mut stdout, inherits, &mut first);
                            }
                            ParseMode::EntityClasses => {
                                todo!();
                            }
                        }
                    },
                    Err(e) => {
                        eprintln!("Unable to read file {}: {}", path.display(), e);
                    }
                }

            }
        }
    }
    println!("\n]");
}

fn print_json_items<T: Serialize, I: IntoIterator<Item = T>, W: Write>(mut out: W, items: I, first: &mut bool) {
    for item in items {
        if !*first {
            writeln!(&mut out, ",").ok();
        }
        write!(&mut out, "\t").ok();
        *first = false;
        serde_json::to_writer(&mut out, &item).expect("Unable to write to stdout");
    }
}

fn parse_file(code: &str) -> (Vec<FoundType>, Vec<Inherit>) {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_cpp::LANGUAGE.into();
    parser
        .set_language(&language)
        .expect("Error loading C++ parser");

    let tree = parser.parse(code.as_bytes(), None).unwrap();
    let fn_declarations = find_key_value_declarations(&language, tree.root_node(), code);

    let mut found_types = Vec::new();
    for f in fn_declarations {
        let matches = find_name_matches(&language, f.body, code);
        for m in matches {
            for (convert_fn, target_type) in CONVERT_FNS {
                let convert_code = m.body.utf8_text(code.as_bytes()).unwrap();
                if convert_code.contains(convert_fn) {
                    found_types.push(FoundType {
                        class: f.name,
                        name: m.name.trim_matches('"'),
                        ty: target_type,
                    })
                }
            }
        }
    }

    for item in find_data_desc_fields(code) {
        found_types.push(item)
    }

    let inherits = find_inherits(&language, tree.root_node(), code);

    (found_types, inherits)
}

#[derive(Debug, Serialize)]
struct FoundType<'code> {
    class: &'code str,
    name: &'code str,
    ty: &'static str,
}

fn find_key_value_declarations<'tree, 'code>(language: &Language, root: Node<'tree>, code: &'code str) -> Vec<FnDeclaration<'tree, 'code>> {
    let query = Query::new(
        language,
        r#"(function_definition
            declarator: (function_declarator declarator: (
                qualified_identifier
                    (namespace_identifier) @class_name
                    (identifier) @fn_name
            ))
            body:(_)  @body
        )"#,
    )
        .expect("invalid query");

    let mut cursor = QueryCursor::new();
    let mut iter = cursor.matches(&query, root, code.as_bytes());
    let mut declarations = vec![];
    while let Some(decl) = iter.next() {
        if decl.captures[1].node.utf8_text(code.as_bytes()).unwrap() == "KeyValue" {
            declarations.push(FnDeclaration {
                name: decl.captures[0].node.utf8_text(code.as_bytes()).unwrap(),
                body: decl.captures[2].node
            })
        }
    }
    declarations
}

struct FnDeclaration<'tree, 'code> {
    name: &'code str,
    body: Node<'tree>,
}

fn find_name_matches<'tree, 'code>(language: &Language, body: Node<'tree>, code: &'code str) -> Vec<NameMatch<'tree, 'code>> {
    let query = Query::new(
        language,
        r#"(if_statement
            condition: (_ value: [
              (binary_expression left: (call_expression
                  function: (identifier) @cmp_fn
                  arguments: (argument_list) @cmp_args
              ))
              (call_expression
                  function: (identifier) @cmp_fn
                  arguments: (argument_list) @cmp_args
              )
            ])
            consequence:(_)  @body
        )"#,
    )
        .expect("invalid query");

    let mut cursor = QueryCursor::new();
    let mut iter = cursor.matches(&query, body, code.as_bytes());
    let mut matches = vec![];
    while let Some(decl) = iter.next() {
        if decl.captures[0].node.utf8_text(code.as_bytes()).unwrap() == "FStrEq" {
            let args = decl.captures[1].node;
            if args.named_child(0).unwrap().utf8_text(code.as_bytes()).unwrap() == "szKeyName" {
                matches.push(NameMatch {
                    name: args.named_child(1).unwrap().utf8_text(code.as_bytes()).unwrap(),
                    body: decl.captures[2].node
                })
            }
        }
    }
    matches
}

struct NameMatch<'tree, 'code> {
    name: &'code str,
    body: Node<'tree>,
}

const CONVERT_FNS: &[(&str, &str)] = &[
    ("if (val)", "bool"),
    ("atoi", "i32"),
    ("UTIL_StringToColor32", "color"),
    ("UTIL_StringToVector", "vector"),
    ("AllocPooledString", "string"),
];


fn find_inherits<'code>(language: &Language, root: Node, code: &'code str) -> Vec<Inherit<'code>> {
    let query = Query::new(
        language,
        r#"(class_specifier
            name: (_) @name
            (base_class_clause (type_identifier) @base)
        )"#,
    )
        .expect("invalid query");

    let mut cursor = QueryCursor::new();
    let mut iter = cursor.matches(&query, root, code.as_bytes());
    let mut declarations = HashMap::new();

    while let Some(decl) = iter.next() {
        let name = decl.captures[0].node.utf8_text(code.as_bytes()).unwrap();
        let inherits = decl.captures[1].node.utf8_text(code.as_bytes()).unwrap();
        let inh = declarations.entry(name).or_insert_with(|| Inherit {
            name,
            inherits: Vec::new(),
        });
        inh.inherits.push(inherits);
    }
    declarations.into_values().collect()
}

#[derive(Debug, Serialize)]
struct Inherit<'code> {
    name: &'code str,
    inherits: Vec<&'code str>,
}

fn find_data_desc_fields(code: &str) -> Vec<FoundType<>> {
    let mut result = Vec::new();
    for (start, _) in code.match_indices("BEGIN_DATADESC(") {
        if let Some(end) = code[start..].find("END_DATADESC") {
            let block = &code[start..start+end];
            let class = block[block.find('(').unwrap()+1..block.find(')').unwrap()].trim();

            for (field_start, _) in block.match_indices("DEFINE_KEYFIELD(") {
                let body = &block[field_start+1..];
                let end = body.find(')').unwrap();
                let body = &body[..end];
                let mut parts = body.split(',').map(str::trim).skip(1);
                if let (Some(ty), Some(name)) = (parts.next(), parts.next()) {
                    let name = name.trim_matches('"');
                    if let Some(ty) = map_type(ty) {
                        result.push(FoundType {
                            class,
                            name,
                            ty
                        });
                    }
                }
            }
        }
    }
    result
}

const TYPE_MAP: &[(&str, &str)] = &[
    ("FIELD_FLOAT", "f32"),
    ("FIELD_STRING", "string"),
    ("FIELD_BOOLEAN", "bool"),
    ("FIELD_INTEGER", "bool"),
    ("FIELD_COLOR32", "color"),
    ("FIELD_VECTOR", "vector"),
];

fn map_type(ty: &str) -> Option<&'static str> {
    for (source_type, target_type) in TYPE_MAP {
        if *source_type == ty {
            return Some(target_type);
        }
    }
    None
}