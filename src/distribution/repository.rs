//! Repository types of the distribution spec.

use bon::Builder;
use getset::{Getters, Setters};
use serde::{Deserialize, Serialize};

#[derive(Builder, Clone, Debug, Deserialize, Eq, Getters, Setters, PartialEq, Serialize)]
#[builder(on(_, into))]
#[getset(get = "pub", set = "pub")]
/// RepositoryList returns a catalog of repositories maintained on the registry.
pub struct RepositoryList {
    /// The items of the RepositoryList.
    repositories: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_list_success() {
        let list = RepositoryList::builder().repositories(vec![]).build();
        assert!(list.repositories().is_empty());
    }
}
