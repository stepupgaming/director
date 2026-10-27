//! Typed computed values. The cascade carries each longhand as its computed
//! text plus the Lightning CSS [`Property`] parsed when the stylesheet loaded,
//! so `style::translate` matches typed values without reparsing text.

use crate::css::Values;
use lightningcss::{
    properties::{
        custom::{TokenList, UnparsedProperty},
        Property, PropertyId,
    },
    stylesheet::ParserOptions,
    traits::IntoOwned,
};
use std::collections::BTreeMap;
use std::ops::Deref;

/// One computed longhand: its text and its parsed value.
#[derive(Clone, Copy, Debug)]
pub struct Computed<'a> {
    pub text: &'a str,
    /// `Property::Unparsed` when Lightning CSS cannot read the text.
    pub value: &'a Property<'static>,
}

/// The computed longhands of one element. Reads through [`Deref`] see the
/// text map; [`ComputedStyle::computed`] adds the typed value.
#[derive(Clone, Debug, Default)]
pub struct ComputedStyle {
    text: Values,
    /// One entry per non-custom name in `text`.
    typed: BTreeMap<String, Property<'static>>,
}

impl ComputedStyle {
    /// Parse every entry of `values`, as captured or authored text arrives.
    pub fn from_text(values: &Values) -> Self {
        let mut style = Self::default();
        for (name, text) in values {
            style.insert(name.clone(), text.clone());
        }
        style
    }

    /// The text and typed value of a non-custom longhand.
    pub fn computed(&self, name: &str) -> Option<Computed<'_>> {
        let (text, value) = (self.text.get(name)?, self.typed.get(name)?);
        Some(Computed { text, value })
    }

    /// Set `name` from text, parsing it.
    pub fn insert(&mut self, name: impl Into<String>, text: impl Into<String>) {
        let name = name.into();
        let text = text.into();
        let property = (!name.starts_with("--")).then(|| parse(&name, &text));
        self.set(name, text, property);
    }

    /// Set `name` from text and the value already parsed from it. Custom
    /// properties carry no typed value.
    pub fn set(&mut self, name: String, text: String, property: Option<Property<'static>>) {
        match property.filter(|_| !name.starts_with("--")) {
            Some(property) => {
                self.typed.insert(name.clone(), property);
            }
            None => {
                self.typed.remove(&name);
            }
        }
        self.text.insert(name, text);
    }

    pub fn remove(&mut self, name: &str) {
        self.text.remove(name);
        self.typed.remove(name);
    }

    /// Copy `name` from `other`, or remove it when `other` has none.
    pub fn copy_from(&mut self, other: &Self, name: &str) {
        match other.text.get(name) {
            Some(text) => self.set(name.into(), text.clone(), other.typed.get(name).cloned()),
            None => self.remove(name),
        }
    }
}

impl Deref for ComputedStyle {
    type Target = Values;

    fn deref(&self) -> &Values {
        &self.text
    }
}

/// Parse one longhand. Text Lightning CSS rejects outright becomes an empty
/// unparsed value, so translation still reports the property.
pub fn parse(name: &str, text: &str) -> Property<'static> {
    Property::parse_string(PropertyId::from(name), text, ParserOptions::default())
        .map(|property| property.into_owned())
        .unwrap_or_else(|_| {
            Property::Unparsed(UnparsedProperty {
                property_id: PropertyId::from(name).into_owned(),
                value: TokenList(Vec::new()),
            })
        })
}
