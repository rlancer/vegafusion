use datafusion::arrow::array::ArrayRef;
use datafusion::arrow::compute::is_not_null;
use datafusion::arrow::datatypes::DataType;
use datafusion::physical_plan::functions::{make_scalar_function, ReturnTypeFunction, Signature};
use datafusion::physical_plan::udf::ScalarUDF;
use std::sync::Arc;

/// `isValid(value)`
///
/// Returns true if value is not null, undefined, or NaN, false otherwise.
///
/// Note: Current implementation does not consider NaN values invalid
///
/// See: https://vega.github.io/vega/docs/expressions/#isValid
pub fn make_is_valid_udf() -> ScalarUDF {
    let is_valid = |args: &[ArrayRef]| {
        // Signature ensures there is a single argument
        let arg = &args[0];
        let result = is_not_null(arg.as_ref()).unwrap();
        Ok(Arc::new(result) as ArrayRef)
    };
    let is_valid = make_scalar_function(is_valid);

    let return_type: ReturnTypeFunction = Arc::new(move |_| Ok(Arc::new(DataType::Boolean)));
    ScalarUDF::new("isValid", &Signature::Any(1), &return_type, &is_valid)
}
