use serde::Deserialize;
use std::collections::HashMap;
use std::iter::once;
use crate::EntityPropertyType;

#[derive(Deserialize, Debug)]
struct EntityClass<'a> {
    entity: &'a str,
    class: &'a str,
}

#[derive(Debug, Deserialize)]
struct Inherit<'a> {
    name: &'a str,
    inherits: Vec<&'a str>,
}

#[derive(Debug, Deserialize)]
struct FoundType<'a> {
    class: &'a str,
    name: &'a str,
    ty: &'a str,
}

pub struct SdkData<'a> {
    classes: Vec<EntityClass<'a>>,
    inherits: Vec<Inherit<'a>>,
    types: Vec<FoundType<'a>>,
}

impl SdkData<'static> {
    pub fn new() -> Self {
        Self::load(
            include_str!("../data/classes.json"),
            include_str!("../data/inherits.json"),
            include_str!("../data/types.json"),
        )
    }
}

impl<'a> SdkData<'a> {
    fn load(classes_json: &'a str, inherits_json: &'a str, types_json: &'a str) -> Self {
        SdkData {
            classes: serde_json::from_str(classes_json).unwrap(),
            inherits: serde_json::from_str(inherits_json).unwrap(),
            types: serde_json::from_str(types_json).unwrap(),
        }
    }

    fn class_for_entity(&self, entity: &str) -> Option<&'a str> {
        self.classes
            .iter()
            .find(|class| class.entity == entity)
            .map(|class| class.class)
    }

    fn inherits_for_class(&'a self, class: &str) -> &'a [&'a str] {
        self.inherits
            .iter()
            .find(|inherit| inherit.name == class)
            .map(|inherit| inherit.inherits.as_slice())
            .unwrap_or_default()
    }

    fn types_for_class(&'a self, class: &'a str) -> impl Iterator<Item = &'a FoundType<'a>> {
        self.types.iter().filter(move |types| types.class == class)
    }

    pub fn types_for_entity(&'a self, entity: &str) -> HashMap<&'a str, EntityPropertyType> {
        let Some(class) = self.class_for_entity(entity) else {
            return HashMap::new();
        };
        let inherits = self.inherits_for_class(class);
        once(class)
            .chain(inherits.iter().copied())
            .flat_map(|class| self.types_for_class(class))
            .map(|ty| (ty.name, map_sdk_type(ty.ty)))
            .collect()
    }
}

fn map_sdk_type(ty: &str) -> EntityPropertyType {
    match ty {
        "color" => EntityPropertyType::Color,
        "vector" => EntityPropertyType::Vector,
        "string" => EntityPropertyType::Str,
        "f32" => EntityPropertyType::F32,
        "i32" => EntityPropertyType::I32,
        "bool" => EntityPropertyType::Bool,
        "angles" => EntityPropertyType::Angles,
        _ => todo!(),
    }
}