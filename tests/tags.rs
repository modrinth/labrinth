use common::environment::TestEnvironment;
use std::collections::{HashMap, HashSet};

mod common;

#[actix_rt::test]
async fn get_tags() {
    let test_env = TestEnvironment::build(None).await;
    let api = &test_env.v3;

    let loaders = api.get_loaders_deserialized().await;
    let categories = api.get_categories_deserialized().await;

    let loader_metadata = loaders
        .into_iter()
        .map(|x| (x.name, x.metadata.get("platform").and_then(|x| x.as_bool())))
        .collect::<HashMap<_, _>>();
    let loader_names = loader_metadata.keys().cloned().collect::<HashSet<String>>();
    assert_eq!(
        loader_names,
        ["fabric", "forge", "mrpack", "bukkit", "waterfall"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    );
    assert_eq!(loader_metadata["fabric"], None);
    assert_eq!(loader_metadata["bukkit"], Some(false));
    assert_eq!(loader_metadata["waterfall"], Some(true));

    let category_names = categories
        .into_iter()
        .map(|x| x.name)
        .collect::<HashSet<_>>();
    assert_eq!(
        category_names,
        [
            "combat",
            "economy",
            "food",
            "optimization",
            "decoration",
            "mobs",
            "magic"
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    );

    test_env.cleanup().await;
}
