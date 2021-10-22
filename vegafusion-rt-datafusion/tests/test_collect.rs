#[macro_use]
extern crate lazy_static;

mod util;

use util::check::check_transform_evaluation;
use util::datasets::vega_json_dataset;
use util::equality::TablesEqualConfig;
use vegafusion_core::spec::transform::collect::{
    CollectSort, CollectTransformSpec, SortOrder, SortOrderOrList,
};
use vegafusion_core::spec::transform::TransformSpec;
use vegafusion_core::spec::values::StringOrStringList;

#[test]
fn test_collect_multi() {
    let dataset = vega_json_dataset("penguins");

    // Apply collect transform
    let collect_spec = CollectTransformSpec {
        sort: CollectSort {
            field: StringOrStringList::StringList(vec![
                "Sex".to_string(),
                "Species".to_string(),
                "Beak Depth (mm)".to_string(),
                "Beak Length (mm)".to_string(),
                "Flipper Length (mm)".to_string(),
            ]),
            order: Some(SortOrderOrList::SortOrderList(vec![
                SortOrder::Ascending,
                SortOrder::Descending,
                SortOrder::Descending,
                SortOrder::Ascending,
                SortOrder::Descending,
            ])),
        },
        extra: Default::default(),
    };

    let transform_specs = vec![TransformSpec::Collect(collect_spec)];

    let comp_config = Default::default();
    let eq_config = TablesEqualConfig {
        row_order: true,
        ..Default::default()
    };

    check_transform_evaluation(
        &dataset,
        transform_specs.as_slice(),
        &comp_config,
        &eq_config,
    );
}
