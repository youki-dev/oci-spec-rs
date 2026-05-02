//! Tag types of the distribution spec.

use bon::Builder;
use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};

#[derive(Builder, Clone, Debug, Deserialize, Eq, Getters, Setters, PartialEq, Serialize)]
#[builder(on(_, into))]
#[getset(get = "pub", set = "pub")]
/// A list of tags for a given repository.
pub struct TagList {
    /// The namespace of the repository.
    name: String,

    /// Each tags on the repository.
    tags: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_list_success() {
        let list = TagList::builder().name("name").tags(vec![]).build();
        assert!(list.tags().is_empty());
        assert_eq!(list.name(), "name");
    }
}
