//! Typed computed values. Each declared longhand parses once into a Lightning
//! CSS [`Property`], so `style::translate` matches typed values.

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

/// One declared longhand: the computed text and its parsed value.
#[derive(Clone, Debug)]
pub struct Computed {
    pub text: String,
    /// `Property::Unparsed` when Lightning CSS cannot read the text.
    pub value: Property<'static>,
}

/// The computed longhands of one element, by name in name order.
#[derive(Clone, Debug, Default)]
pub struct ComputedStyle {
    entries: BTreeMap<String, Computed>,
}

impl ComputedStyle {
    /// Parse every value of `values`. Custom properties are left out.
    pub fn parse(values: &Values) -> Self {
        let entries = values
            .iter()
            .filter(|(name, _)| !name.starts_with("--"))
            .map(|(name, text)| (name.clone(), computed(name, text)))
            .collect();
        Self { entries }
    }

    pub fn get(&self, name: &str) -> Option<&Computed> {
        self.entries.get(name)
    }
}

fn computed(name: &str, text: &str) -> Computed {
    let value = Property::parse_string(PropertyId::from(name), text, ParserOptions::default())
        .map(|property| property.into_owned())
        .unwrap_or_else(|_| unparsed(name));
    Computed {
        text: text.to_string(),
        value,
    }
}

/// An empty unparsed value for text Lightning CSS rejects outright.
fn unparsed(name: &str) -> Property<'static> {
    Property::Unparsed(UnparsedProperty {
        property_id: PropertyId::from(name).into_owned(),
        value: TokenList(Vec::new()),
    })
}
