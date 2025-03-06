use clap::{Args, Parser, Subcommand};
use quote::ToTokens;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use vbsp::EntityProp;

use vbsp::{Angles, Color, LightColor, Negated, Vector};

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Generate(command) => {
            bsp_entities(command.input_files, command.output_file).unwrap()
        }
    }
}

#[derive(Parser)]
#[command(author,version,about,long_about=None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Generate(GenerateSubcommand),
    // Config(ConfigSubcommand),
}

/// Generate entity structs for a specified list of files.
#[derive(Args)]
struct GenerateSubcommand {
    #[arg(long, short)]
    output_file: PathBuf,
    input_files: Vec<PathBuf>,
}

/// Generate entity structs using a configured preset.
// #[derive(Args)]
// struct ConfigSubcommand{
// 	input_files:Vec<PathBuf>,
// }

#[allow(dead_code)]
#[derive(Debug)]
enum ReadBspError {
    Io(std::io::Error),
    Bsp(vbsp::BspError),
}
impl std::fmt::Display for ReadBspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadBspError::Io(error) => write!(f, "Io: {}", error),
            ReadBspError::Bsp(bsp_error) => write!(f, "Bsp: {}", bsp_error),
        }
    }
}
impl std::error::Error for ReadBspError {}

fn read_bsp(path: PathBuf) -> Result<vbsp::Bsp, ReadBspError> {
    let entire_file = std::fs::read(path).map_err(ReadBspError::Io)?;
    let bsp = vbsp::Bsp::read(&entire_file).map_err(ReadBspError::Bsp)?;
    Ok(bsp)
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
enum EntityPropertyType {
    Bool,
    Negated,
    U8,
    // I8,
    U16,
    // I16,
    U32,
    I32,
    F32,
    Color,
    LightColor,
    Angles,
    Vector,
    Str,
}

impl EntityPropertyType {
    const VARIANT_COUNT: usize = 12;
    fn codegen(&self, name: &str, optional: bool) -> syn::Field {
        let (mut attrs, ty) = match self {
            EntityPropertyType::Bool => (
                vec![syn::parse_quote!(#[serde(deserialize_with = "deserialize_bool")])],
                // no such thing as Option<bool>
                syn::parse_quote!(bool),
            ),
            EntityPropertyType::Negated => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<Negated>)
                } else {
                    syn::parse_quote!(Negated)
                },
            ),
            EntityPropertyType::U8 => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<u8>)
                } else {
                    syn::parse_quote!(u8)
                },
            ),
            EntityPropertyType::U16 => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<u16>)
                } else {
                    syn::parse_quote!(u16)
                },
            ),
            EntityPropertyType::U32 => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<u32>)
                } else {
                    syn::parse_quote!(u32)
                },
            ),
            EntityPropertyType::I32 => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<i32>)
                } else {
                    syn::parse_quote!(i32)
                },
            ),
            EntityPropertyType::F32 => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<f32>)
                } else {
                    syn::parse_quote!(f32)
                },
            ),
            EntityPropertyType::Color => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<Color>)
                } else {
                    syn::parse_quote!(Color)
                },
            ),
            EntityPropertyType::LightColor => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<LightColor>)
                } else {
                    syn::parse_quote!(LightColor)
                },
            ),
            EntityPropertyType::Angles => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<Angles>)
                } else {
                    syn::parse_quote!(Angles)
                },
            ),
            EntityPropertyType::Vector => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<Vector>)
                } else {
                    syn::parse_quote!(Vector)
                },
            ),
            EntityPropertyType::Str => (
                vec![],
                if optional {
                    syn::parse_quote!(Option<&'a str>)
                } else {
                    syn::parse_quote!(&'a str)
                },
            ),
        };

        if optional {
            attrs.push(syn::parse_quote!(#[serde(default)]));
        }

        let ident = match syn::parse_str(name) {
            Ok(ident) => ident,
            Err(_) => {
                if name == "type" {
                    syn::parse_quote!(r#type)
                } else {
                    attrs.push(syn::parse_quote!(#[serde(rename = #name)]));
                    let new_name = name.replace('.', "_");
                    syn::Ident::new(&new_name, proc_macro2::Span::call_site())
                }
            }
        };
        syn::Field {
            attrs,
            vis: syn::Visibility::Public(syn::token::Pub::default()),
            mutability: syn::FieldMutability::None,
            ident: Some(ident),
            colon_token: Some(syn::token::Colon::default()),
            ty,
        }
    }
}

fn get_bool(value: &str) -> Option<bool> {
    match value {
        "0" | "no" => Some(false),
        "1" | "yes" => Some(true),
        _ => None,
    }
}
fn get_minimal_type(name: &str, values: &[&str]) -> EntityPropertyType {
    let mut max_count = 0;
    // Track how many property values parse successfully for each type.
    let mut counts = Vec::with_capacity(EntityPropertyType::VARIANT_COUNT);
    if !matches!(name, "spawnflags" | "ammo") {
        let count = values.iter().flat_map(|&v| get_bool(v)).count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::Bool, count));
        if count == values.len() {
            return EntityPropertyType::Bool;
        }
    }
    if !matches!(name, "spawnflags" | "ammo") {
        let count = values.iter().flat_map(|&v| v.parse::<Negated>()).count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::Negated, count));
        if count == values.len() {
            return EntityPropertyType::Negated;
        }
    }
    if !matches!(name, "spawnflags" | "ammo") {
        let count = values
            .iter()
            .flat_map(|&v| <u8 as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::U8, count));
        if count == values.len() {
            return EntityPropertyType::U8;
        }
    }
    // if values.iter().all(|&v|<i8 as EntityProp>::parse(v).is_ok()){
    // 	let count=values.iter().flat_map(|&v|<u8 as EntityProp>::parse(v)).count();
    // 	max_count=max_count.max(count);
    // 	if count==values.len(){
    // 		return EntityPropertyType::U8;
    // 	}
    // 	return EntityPropertyType::I8;
    // }
    if name != "spawnflags" {
        let count = values
            .iter()
            .flat_map(|&v| <u16 as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::U16, count));
        if count == values.len() {
            return EntityPropertyType::U16;
        }
    }
    // if values.iter().all(|&v|<i16 as EntityProp>::parse(v).is_ok()){
    // 	let count=values.iter().flat_map(|&v|<u8 as EntityProp>::parse(v)).count();
    // 	max_count=max_count.max(count);
    // 	if count==values.len(){
    // 		return EntityPropertyType::U8;
    // 	}
    // 	return EntityPropertyType::I16;
    // }
    {
        let count = values
            .iter()
            .flat_map(|&v| <u32 as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::U32, count));
        if count == values.len() {
            return EntityPropertyType::U32;
        }
    }
    {
        let count = values
            .iter()
            .flat_map(|&v| <i32 as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::I32, count));
        if count == values.len() {
            return EntityPropertyType::I32;
        }
    }
    {
        let count = values
            .iter()
            .flat_map(|&v| <f32 as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::F32, count));
        if count == values.len() {
            return EntityPropertyType::F32;
        }
    }
    if name.find("color").is_some()
        || name.find("light").is_some()
        || name.find("ambient").is_some()
    {
        let count = values
            .iter()
            .flat_map(|&v| <Color as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::Color, count));
        if count == values.len() {
            return EntityPropertyType::Color;
        }
    }
    {
        let count = values
            .iter()
            .flat_map(|&v| <LightColor as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::LightColor, count));
        if count == values.len() {
            return EntityPropertyType::LightColor;
        }
    }
    if name.find("angles").is_some() || name.ends_with("dir") {
        let count = values
            .iter()
            .flat_map(|&v| <Angles as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::Angles, count));
        if count == values.len() {
            return EntityPropertyType::Angles;
        }
    }
    {
        let count = values
            .iter()
            .flat_map(|&v| <Vector as EntityProp>::parse(v))
            .count();
        max_count = max_count.max(count);
        counts.push((EntityPropertyType::Vector, count));
        if count == values.len() {
            return EntityPropertyType::Vector;
        }
    }
    if 1 < values.len() && values.len() / 2 < max_count {
        // why are there outliers that fail to parse?
        let unique_values: HashSet<_> = values.iter().copied().collect();
        println!("{name}: over 50% parsed, inspect outliers: {counts:?}\n{unique_values:?}",);

        if values.len() as f32 * 0.99 < max_count as f32 {
            println!("over 99% parsed, accepting even with outliers");
            // This assumes that counts is sorted by EntityPropertyType.
            for (ty, count) in counts {
                if count == max_count {
                    return ty;
                }
            }
        }
    }
    EntityPropertyType::Str
}

struct ClassCollector<'a> {
    occurrences: usize,
    values: HashMap<&'a str, Vec<&'a str>>,
}

#[allow(dead_code)]
#[derive(Debug)]
enum BspEntitiesError {
    ReadBsp(ReadBspError),
    Io(std::io::Error),
    FormatFailed,
}

fn bsp_entities(paths: Vec<PathBuf>, dest: PathBuf) -> Result<(), BspEntitiesError> {
    let start = std::time::Instant::now();

    // decode bsps in parallel using available_parallelism
    let bsps_entities = {
        let mut bsps = Vec::with_capacity(paths.len());

        let thread_limit = std::thread::available_parallelism()
            .map_err(BspEntitiesError::Io)?
            .get();
        let mut threads = std::collections::VecDeque::with_capacity(thread_limit);
        type Thread = std::thread::JoinHandle<(PathBuf, Result<vbsp::Bsp, ReadBspError>)>;
        let mut join_thread = |thread: Thread| match thread.join() {
            Ok((_, Ok(bsp))) => Ok(bsps.push(bsp.entities)),
            Ok((path, Err(e))) => Ok(println!("File={:?} ReadBsp error: {}", path.file_stem(), e)),
            Err(e) => Err(e),
        };

        for path in paths {
            if thread_limit <= threads.len() {
                join_thread(threads.pop_front().unwrap()).unwrap();
            }
            threads.push_back(std::thread::spawn(|| (path.clone(), read_bsp(path))));
        }

        for thread in threads {
            join_thread(thread).unwrap();
        }

        bsps
    };

    let decode_elapsed = start.elapsed();
    println!(
        "bsps decoded={} elapsed={:?}",
        bsps_entities.len(),
        decode_elapsed
    );

    let start_generate = std::time::Instant::now();

    // collect observed class instances
    let mut classes = std::collections::HashMap::new();
    for entities in &bsps_entities {
        for ent in entities {
            if let Some(class) = ent.prop("classname") {
                if class == "" {
                    println!("empty class ident! class={class}");
                    continue;
                }
                let props = classes.entry(class).or_insert(ClassCollector {
                    occurrences: 0,
                    values: HashMap::new(),
                });
                props.occurrences += 1;
                for (name, value) in ent.properties() {
                    if matches!(name, "classname" | "hammerid") {
                        continue;
                    }
                    if name == "" {
                        println!("empty ident! class={class} value={value}");
                        continue;
                    }
                    let values = props.values.entry(name).or_insert(Vec::new());
                    // observed value string
                    values.push(value);
                }
            } else {
                println!("No classname in entity! {ent:?}");
            }
        }
    }

    // generate a struct for each entity
    let mut entity_structs = Vec::new();
    let mut entity_variants = Vec::new();
    for (classname, properties) in classes {
        let mut has_lifetime = false;
        let mut props = Vec::new();
        for (propname, values) in properties.values {
            // exhaustively make sure all observed values can be parsed by the chosen type
            let ty = get_minimal_type(propname, &values);
            if matches!(ty, EntityPropertyType::Str) {
                has_lifetime = true;
            }
            // this is an optional type and should have a default value
            let optional = values.len() < properties.occurrences;
            props.push(ty.codegen(propname, optional));
        }
        // sort props for consistency
        props.sort_by(|a, b| a.ident.cmp(&b.ident));

        // struct ident in UpperCamelCase
        let ident = syn::Ident::new(
            &heck::ToUpperCamelCase::to_upper_camel_case(classname),
            proc_macro2::Span::call_site(),
        );

        // generate the class struct with all observed fields
        entity_structs.push(syn::ItemStruct {
            attrs: vec![syn::parse_quote!(#[derive(Debug, Clone, Deserialize)])],
            vis: syn::Visibility::Public(syn::token::Pub::default()),
            struct_token: syn::token::Struct::default(),
            ident: ident.clone(),
            generics: if has_lifetime {
                syn::parse_quote!(<'a>)
            } else {
                syn::parse_quote!()
            },
            fields: syn::Fields::Named(syn::FieldsNamed {
                brace_token: syn::token::Brace::default(),
                named: props.into_iter().collect(),
            }),
            semi_token: None,
        });

        // generate Entities enum variant
        let arguments = if has_lifetime {
            syn::PathArguments::AngleBracketed(syn::parse_quote!(<'a>))
        } else {
            syn::PathArguments::None
        };
        let mut attrs = vec![syn::parse_quote!(#[serde(rename = #classname)])];
        if has_lifetime {
            attrs.push(syn::parse_quote!(#[serde(borrow)]));
        }
        entity_variants.push(syn::Variant {
            attrs,
            ident: ident.clone(),
            fields: syn::Fields::Unnamed(syn::FieldsUnnamed {
                paren_token: syn::token::Paren::default(),
                unnamed: [syn::Field {
                    attrs: vec![],
                    vis: syn::Visibility::Inherited,
                    mutability: syn::FieldMutability::None,
                    ident: None,
                    colon_token: None,
                    ty: syn::Type::Path(syn::TypePath {
                        qself: None,
                        path: syn::Path {
                            leading_colon: None,
                            segments: [syn::PathSegment { ident, arguments }]
                                .into_iter()
                                .collect(),
                        },
                    }),
                }]
                .into_iter()
                .collect(),
            }),
            discriminant: None,
        });
    }

    // sort entities for consistency
    entity_structs.sort_by(|a, b| a.ident.cmp(&b.ident));
    entity_variants.sort_by(|a, b| a.ident.cmp(&b.ident));

    // generate entities enum
    let mut entities_enum: syn::ItemEnum = syn::parse_quote! {
        #[derive(Debug, Clone, Deserialize)]
        #[non_exhaustive]
        #[serde(tag = "classname")]
        pub enum Entity<'a> {
        }
    };
    entities_enum.variants.extend(entity_variants);

    // create complete file including use statements
    let mut complete_file: syn::File = syn::parse_quote! {
        use serde::Deserialize;
        use vbsp_common::deserialize_bool;
        use vbsp_common::{Angles, Color, LightColor, Negated, Vector};
    };
    complete_file.items.push(syn::Item::Enum(entities_enum));
    complete_file
        .items
        .extend(entity_structs.into_iter().map(syn::Item::Struct));

    // time!
    let generate_elapsed = start_generate.elapsed();
    let start_format = std::time::Instant::now();

    // make a string of the unformatted code
    let code = complete_file.into_token_stream().to_string();

    // format via cli
    let cmd = Command::new("rustfmt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(BspEntitiesError::Io)?;
    cmd.stdin
        .as_ref()
        .unwrap()
        .write_all(code.as_bytes())
        .map_err(BspEntitiesError::Io)?;
    let output = cmd.wait_with_output().map_err(BspEntitiesError::Io)?;

    if !output.status.success() {
        return Err(BspEntitiesError::FormatFailed);
    }

    let format_elapsed = start_format.elapsed();
    let start_output = std::time::Instant::now();

    // save to destination file
    let mut file = std::fs::File::create(dest).map_err(BspEntitiesError::Io)?;
    file.write_all(&output.stdout)
        .map_err(BspEntitiesError::Io)?;

    let output_elapsed = start_output.elapsed();
    let elapsed = start.elapsed();

    // TODO: use steamlocate to find tf2 maps

    println!("decode elapsed={decode_elapsed:?}");
    println!("generate elapsed={generate_elapsed:?}");
    println!("format elapsed={format_elapsed:?}");
    println!("output elapsed={output_elapsed:?}");
    println!("total elapsed={elapsed:?}");
    Ok(())
}

// auxilliary function to sort existing structs
fn _sort_structs() {
    let mut file: syn::File = syn::parse_quote! {
        // PASTE STRUCTS HERE TO SORT THEM
    };

    for item in &mut file.items {
        if let syn::Item::Struct(s) = item {
            let mut fields: Vec<_> = s.fields.iter().cloned().collect();
            fields.sort_by(|a, b| a.ident.cmp(&b.ident));
            s.fields = syn::Fields::Named(syn::FieldsNamed {
                brace_token: syn::token::Brace::default(),
                named: fields.into_iter().collect(),
            });
        }
    }

    file.items.sort_by(|a, b| {
        if let (syn::Item::Struct(a), syn::Item::Struct(b)) = (a, b) {
            a.ident.cmp(&b.ident)
        } else {
            panic!();
        }
    });

    println!("{}", file.into_token_stream().to_string());
}

// auxilliary function to sort existing enum variants
fn _sort_enum() {
    let mut entities_enum: syn::ItemEnum = syn::parse_quote! {
        // PASTE ENUM HERE TO SORT VARIANTS
    };

    let mut variants: Vec<_> = entities_enum.variants.iter().cloned().collect();
    variants.sort_by(|a, b| a.ident.cmp(&b.ident));
    entities_enum.variants = variants.into_iter().collect();

    println!("{}", entities_enum.into_token_stream().to_string());
}
