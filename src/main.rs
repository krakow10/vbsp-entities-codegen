use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use quote::ToTokens;
use vbsp::EntityProp;

use vbsp::{Angles,Color,LightColor,Vector};

fn main() {
	let paths=std::env::args().map(PathBuf::from).collect();
	bsp_entities(paths).unwrap();
}

#[allow(dead_code)]
#[derive(Debug)]
enum ReadBspError{
	Io(std::io::Error),
	Bsp(vbsp::BspError),
}

fn read_bsp(path:PathBuf)->Result<vbsp::Bsp,ReadBspError>{
	let entire_file=std::fs::read(path).map_err(ReadBspError::Io)?;
	let bsp=vbsp::Bsp::read(&entire_file).map_err(ReadBspError::Bsp)?;
	Ok(bsp)
}

enum EntityPropertyType{
	Bool,
	U8,
	// I8,
	U16,
	// I16,
	U32,
	I32,
	F32,
	Color,
	LightColor,
	Vector,
	Angles,
	Str,
}

impl EntityPropertyType{
	fn codegen(&self,name:&str,optional:bool)->syn::Field{
		let (mut attrs,ty)=match self{
			EntityPropertyType::Bool=>(
				vec![syn::parse_quote!(#[serde(deserialize_with = "bool_from_int")])],
				// no such thing as Option<bool>
				syn::parse_quote!(bool)
			),
			EntityPropertyType::U8=>(vec![],if optional{syn::parse_quote!(Option<u8>)}else{syn::parse_quote!(u8)}),
			EntityPropertyType::U16=>(vec![],if optional{syn::parse_quote!(Option<u16>)}else{syn::parse_quote!(u16)}),
			EntityPropertyType::U32=>(vec![],if optional{syn::parse_quote!(Option<u32>)}else{syn::parse_quote!(u32)}),
			EntityPropertyType::I32=>(vec![],if optional{syn::parse_quote!(Option<i32>)}else{syn::parse_quote!(i32)}),
			EntityPropertyType::F32=>(vec![],if optional{syn::parse_quote!(Option<f32>)}else{syn::parse_quote!(f32)}),
			EntityPropertyType::Color=>(vec![],if optional{syn::parse_quote!(Option<Color>)}else{syn::parse_quote!(Color)}),
			EntityPropertyType::LightColor=>(vec![],if optional{syn::parse_quote!(Option<LightColor>)}else{syn::parse_quote!(LightColor)}),
			EntityPropertyType::Vector=>(vec![],if optional{syn::parse_quote!(Option<Vector>)}else{syn::parse_quote!(Vector)}),
			EntityPropertyType::Angles=>(vec![],if optional{syn::parse_quote!(Option<Angles>)}else{syn::parse_quote!(Angles)}),
			EntityPropertyType::Str=>(vec![],if optional{syn::parse_quote!(Option<&'a str>)}else{syn::parse_quote!(&'a str)}),
		};

		if optional{
			attrs.push(syn::parse_quote!(#[serde(default)]));
		}

		let ident=match syn::parse_str(name){
			Ok(ident)=>ident,
			Err(_)=>{
				if name=="type"{
					syn::parse_quote!(r#type)
				}else{
					attrs.push(syn::parse_quote!(#[serde(rename = #name)]));
					let new_name=name.replace('.',"_");
					syn::Ident::new(&new_name,proc_macro2::Span::call_site())
				}
			}
		};
		syn::Field{
			attrs,
			vis:syn::Visibility::Public(syn::token::Pub::default()),
			mutability:syn::FieldMutability::None,
			ident:Some(ident),
			colon_token:Some(syn::token::Colon::default()),
			ty,
		}
	}
}

fn get_bool(value:&str)->Option<bool>{
	match value{
		"0"=>Some(false),
		"1"=>Some(true),
		_=>None
	}
}
fn get_minimal_type(name:&str,values:&[&str])->EntityPropertyType{
	if !matches!(name,"spawnflags"|"ammo")&&values.iter().all(|&v|get_bool(v).is_some()){
		return EntityPropertyType::Bool;
	}
	if !matches!(name,"spawnflags"|"ammo")&&values.iter().all(|&v|<u8 as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::U8;
	}
	// if values.iter().all(|&v|<i8 as EntityProp>::parse(v).is_ok()){
	// 	return EntityPropertyType::I8;
	// }
	if name!="spawnflags"&&values.iter().all(|&v|<u16 as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::U16;
	}
	// if values.iter().all(|&v|<i16 as EntityProp>::parse(v).is_ok()){
	// 	return EntityPropertyType::I16;
	// }
	if values.iter().all(|&v|<u32 as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::U32;
	}
	if values.iter().all(|&v|<i32 as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::I32;
	}
	if values.iter().all(|&v|<f32 as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::F32;
	}
	if (
		name.find("color").is_some()
		||name.find("light").is_some()
		||name.find("ambient").is_some()
	)&&values.iter().all(|&v|<Color as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::Color;
	}
	if values.iter().all(|&v|<LightColor as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::LightColor;
	}
	if name.find("angles").is_some()&&values.iter().all(|&v|<Angles as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::Angles;
	}
	if values.iter().all(|&v|<Vector as EntityProp>::parse(v).is_ok()){
		return EntityPropertyType::Vector;
	}
	EntityPropertyType::Str
}

struct ClassCollector<'a>{
	occurrences:usize,
	values:HashMap<&'a str,Vec<&'a str>>
}

#[allow(dead_code)]
#[derive(Debug)]
enum BspEntitiesError{
	ReadBsp(ReadBspError),
	Io(std::io::Error),
}

fn bsp_entities(paths:Vec<std::path::PathBuf>)->Result<(),BspEntitiesError>{
	let start=std::time::Instant::now();

	// decode bsps in parallel using available_parallelism
	let bsps={
		let mut bsps=Vec::with_capacity(paths.len());

		let thread_limit=std::thread::available_parallelism().map_err(BspEntitiesError::Io)?.get();
		let mut threads=std::collections::VecDeque::with_capacity(thread_limit);
		type Thread=std::thread::JoinHandle<Result<vbsp::Bsp,ReadBspError>>;
		let mut join_thread=|thread:Thread|{
			match thread.join(){
				Ok(Ok(bsp))=>Ok(bsps.push(bsp)),
				Ok(Err(e))=>Ok(println!("ReadBsp error: {:?}",e)),
				Err(e)=>Err(e),
			}
		};

		for path in paths{
			if thread_limit<=threads.len(){
				join_thread(threads.pop_front().unwrap()).unwrap();
			}
			threads.push_back(std::thread::spawn(||
				read_bsp(path)
			));
		}

		for thread in threads{
			join_thread(thread).unwrap();
		}

		// save 2kB of memory :scream:
		bsps.into_boxed_slice()
	};
	println!("bsps decoded={} elapsed={:?}",bsps.len(),start.elapsed());

	let start_convert=std::time::Instant::now();

	// collect observed class instances
	let mut classes=std::collections::HashMap::new();
	for bsp in &bsps{
		for ent in &bsp.entities{
			if let Some(class)=ent.prop("classname"){
				let props=classes.entry(class).or_insert(ClassCollector{occurrences:0,values:HashMap::new()});
				props.occurrences+=1;
				for (name,value) in ent.properties(){
					if matches!(name,"classname"|"hammerid"){
						continue;
					}
					if name==""{
						println!("empty ident! class={class} value={value}");
						continue;
					}
					let values=props.values.entry(name).or_insert(Vec::new());
					// observed value string
					values.push(value);
				}
			}else{
				println!("No classname in entity! {ent:?}");
			}
		}
	}

	// generate a struct for each entity
	let mut entity_structs=Vec::new();
	let mut entity_variants=Vec::new();
	for (classname,properties) in classes{
		let mut has_lifetime=false;
		let mut props=Vec::new();
		for (propname,values) in properties.values{
			// exhaustively make sure all observed values can be parsed by the chosen type
			let ty=get_minimal_type(propname,&values);
			if matches!(ty,EntityPropertyType::Str){
				has_lifetime=true;
			}
			// this is an optional type and should have a default value
			let optional=values.len()<properties.occurrences;
			props.push(ty.codegen(propname,optional));
		}
		// sort props for consistency
		props.sort_by(|a,b|a.ident.cmp(&b.ident));

		// struct ident in UpperCamelCase
		let ident=syn::Ident::new(&heck::ToUpperCamelCase::to_upper_camel_case(classname),proc_macro2::Span::call_site());

		// generate the class struct with all observed fields
		entity_structs.push(syn::ItemStruct{
			attrs:vec![syn::parse_quote!(#[derive(Debug, Clone, Deserialize)])],
			vis:syn::Visibility::Public(syn::token::Pub::default()),
			struct_token:syn::token::Struct::default(),
			ident:ident.clone(),
			generics:if has_lifetime{syn::parse_quote!(<'a>)}else{syn::parse_quote!()},
			fields:syn::Fields::Named(syn::FieldsNamed{brace_token:syn::token::Brace::default(),named:props.into_iter().collect()}),
			semi_token:None,
		});

		// generate Entities enum variant
		let arguments=if has_lifetime{
			syn::PathArguments::AngleBracketed(syn::parse_quote!(<'a>))
		}else{
			syn::PathArguments::None
		};
		let mut attrs=vec![syn::parse_quote!(#[serde(rename = #classname)])];
		if has_lifetime{
			attrs.push(syn::parse_quote!(#[serde(borrow)]));
		}
		entity_variants.push(syn::Variant{
			attrs,
			ident:ident.clone(),
			fields:syn::Fields::Unnamed(syn::FieldsUnnamed{paren_token:syn::token::Paren::default(),unnamed:[syn::Field{
				attrs:vec![],
				vis:syn::Visibility::Inherited,
				mutability:syn::FieldMutability::None,
				ident:None,
				colon_token:None,
				ty:syn::Type::Path(syn::TypePath{qself:None,path:syn::Path{leading_colon:None,segments:[syn::PathSegment{ident,arguments}].into_iter().collect()}}),
			}].into_iter().collect()}),
			discriminant:None,
		});
	}

	// sort entities for consistency
	entity_structs.sort_by(|a,b|a.ident.cmp(&b.ident));
	entity_variants.sort_by(|a,b|a.ident.cmp(&b.ident));

	// generate entities enum
	let mut entities_enum:syn::ItemEnum=syn::parse_quote!{
		#[derive(Debug, Clone, Deserialize)]
		#[non_exhaustive]
		#[serde(tag = "classname")]
		pub enum Entity<'a> {
		}
	};
	entities_enum.variants.extend(entity_variants);

	// time!
	let convert_elapsed=start_convert.elapsed();
	let elapsed=start.elapsed();

	// print that sucker out
	// save to codegen.rs
	let mut file=std::fs::File::create("codegen.rs").map_err(BspEntitiesError::Io)?;
	file.write(entities_enum.into_token_stream().to_string().as_bytes()).map_err(BspEntitiesError::Io)?;

	for entity_struct in entity_structs{
		file.write(entity_struct.into_token_stream().to_string().as_bytes()).map_err(BspEntitiesError::Io)?;
	}

	println!("convert elapsed={:?}",convert_elapsed);
	println!("total elapsed={:?}",elapsed);
	Ok(())
}

// auxilliary function to sort existing structs
fn _sort_structs(){
	let mut file:syn::File=syn::parse_quote!{
		// PASTE STRUCTS HERE TO SORT THEM
	};

	for item in &mut file.items{
		if let syn::Item::Struct(s)=item{
			let mut fields:Vec<_>=s.fields.iter().cloned().collect();
			fields.sort_by(|a,b|a.ident.cmp(&b.ident));
			s.fields=syn::Fields::Named(syn::FieldsNamed{brace_token:syn::token::Brace::default(),named:fields.into_iter().collect()});
		}
	}

	file.items.sort_by(|a,b|{
		if let (syn::Item::Struct(a),syn::Item::Struct(b))=(a,b){
			a.ident.cmp(&b.ident)
		}else{
			panic!();
		}
	});

	println!("{}",file.into_token_stream().to_string());
}

// auxilliary function to sort existing enum variants
fn _sort_enum(){
	let mut entities_enum:syn::ItemEnum=syn::parse_quote!{
		// PASTE ENUM HERE TO SORT VARIANTS
	};

	let mut variants:Vec<_>=entities_enum.variants.iter().cloned().collect();
	variants.sort_by(|a,b|a.ident.cmp(&b.ident));
	entities_enum.variants=variants.into_iter().collect();

	println!("{}",entities_enum.into_token_stream().to_string());
}
