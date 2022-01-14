/*
 * VegaFusion
 * Copyright (C) 2022 Jon Mease
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public
 * License along with this program.
 * If not, see http://www.gnu.org/licenses/.
 */
use crate::expression::compiler::builtin_functions::date_time::date_parsing::{
    datetime_strs_to_millis, DateParseMode,
};

use datafusion::arrow::array::{
    Array, ArrayRef, Date32Array, Date64Array, Int64Array, StringArray,
};
use datafusion::arrow::compute::cast;
use datafusion::arrow::datatypes::{DataType, TimeUnit};
use datafusion::physical_plan::functions::{
    make_scalar_function, ReturnTypeFunction, Signature, Volatility,
};
use datafusion::physical_plan::udf::ScalarUDF;
use std::sync::Arc;
use time::{OffsetDateTime, UtcOffset};
use vegafusion_core::arrow::compute::unary;

#[inline(always)]
pub fn extract_year(dt: &OffsetDateTime) -> i64 {
    dt.year() as i64
}

#[inline(always)]
pub fn extract_month(dt: &OffsetDateTime) -> i64 {
    (dt.month() as i64) - 1
}

#[inline(always)]
pub fn extract_quarter(dt: &OffsetDateTime) -> i64 {
    let month0 = (dt.month() as u8 as f64) - 1.0;
    (month0 / 3.0).floor() as i64 + 1
}

#[inline(always)]
pub fn extract_date(dt: &OffsetDateTime) -> i64 {
    dt.day() as i64
}

#[inline(always)]
pub fn extract_day(dt: &OffsetDateTime) -> i64 {
    dt.weekday().number_days_from_sunday() as i64
}

#[inline(always)]
pub fn extract_dayofyear(dt: &OffsetDateTime) -> i64 {
    dt.ordinal() as i64
}

#[inline(always)]
pub fn extract_hour(dt: &OffsetDateTime) -> i64 {
    dt.hour() as i64
}

#[inline(always)]
pub fn extract_minute(dt: &OffsetDateTime) -> i64 {
    dt.minute() as i64
}

#[inline(always)]
pub fn extract_second(dt: &OffsetDateTime) -> i64 {
    dt.second() as i64
}

#[inline(always)]
pub fn extract_millisecond(dt: &OffsetDateTime) -> i64 {
    dt.millisecond() as i64
}

pub fn make_datepart_udf(
    extract_fn: fn(&OffsetDateTime) -> i64,
    local: bool,
    name: &str,
) -> ScalarUDF {
    let part_fn = move |args: &[ArrayRef]| {
        // Signature ensures there is a single argument
        let arg = &args[0];

        let arg = match arg.data_type() {
            DataType::Utf8 => {
                let array = arg.as_any().downcast_ref::<StringArray>().unwrap();
                let millis_array = datetime_strs_to_millis(array, DateParseMode::JavaScript);
                cast(&millis_array, &DataType::Date64)?
            }
            DataType::Timestamp(TimeUnit::Millisecond, _) => cast(arg, &DataType::Date64)?,
            DataType::Date32 => {
                let ms_per_day = 1000 * 60 * 60 * 24_i64;
                let array = arg.as_any().downcast_ref::<Date32Array>().unwrap();

                let array: Int64Array = unary(array, |v| (v as i64) * ms_per_day);
                let array = Arc::new(array) as ArrayRef;
                cast(&array, &DataType::Date64)?
            }
            DataType::Date64 => arg.clone(),
            DataType::Int64 => cast(arg, &DataType::Date64)?,
            _ => panic!("Unexpected data type for date part function:"),
        };

        let arg = arg.as_any().downcast_ref::<Date64Array>().unwrap();

        let mut result_builder = Int64Array::builder(arg.len());

        if local {
            // Work in Local
            for i in 0..arg.len() {
                if arg.is_null(i) {
                    result_builder.append_null().unwrap();
                } else {
                    // Still interpret timestamp as UTC
                    let utc_seconds = arg.value(i) / 1000;
                    let utc_datetime = OffsetDateTime::from_unix_timestamp(utc_seconds)
                        .expect("Failed to convert timestamp to OffsetDateTime");

                    let offset = time::UtcOffset::local_offset_at(utc_datetime)
                        .expect("Failed to determine local timezone");
                    let local_datetime = utc_datetime.to_offset(offset);
                    let value = extract_fn(&local_datetime);
                    result_builder.append_value(value).unwrap();
                }
            }
        } else {
            // Work in UTC
            for i in 0..arg.len() {
                if arg.is_null(i) {
                    result_builder.append_null().unwrap();
                } else {
                    let utc_seconds = arg.value(i) / 1000;
                    let utc_datetime = OffsetDateTime::from_unix_timestamp(utc_seconds)
                        .expect("Failed to convert timestamp to OffsetDateTime");
                    let value = extract_fn(&utc_datetime);
                    result_builder.append_value(value).unwrap();
                }
            }
        }

        Ok(Arc::new(result_builder.finish()) as ArrayRef)
    };
    let part_fn = make_scalar_function(part_fn);

    let return_type: ReturnTypeFunction = Arc::new(move |_| Ok(Arc::new(DataType::Int64)));
    ScalarUDF::new(
        name,
        &Signature::uniform(
            1,
            vec![
                DataType::Utf8,
                DataType::Timestamp(TimeUnit::Millisecond, None),
                DataType::Date32,
                DataType::Date64,
                DataType::Int64,
            ],
            Volatility::Immutable,
        ),
        &return_type,
        &part_fn,
    )
}

lazy_static! {
    // Local
    pub static ref YEAR_UDF: ScalarUDF =
        make_datepart_udf(extract_year, true, "year");
    pub static ref MONTH_UDF: ScalarUDF =
        make_datepart_udf(extract_month, true, "month");
    pub static ref QUARTER_UDF: ScalarUDF =
        make_datepart_udf(extract_quarter, true, "quarter");
    pub static ref DATE_UDF: ScalarUDF =
        make_datepart_udf(extract_date, true, "date");
    pub static ref DAYOFYEAR_UDF: ScalarUDF =
        make_datepart_udf(extract_dayofyear, true, "dayofyear");
    pub static ref DAY_UDF: ScalarUDF =
        make_datepart_udf(extract_day, true, "day");
    pub static ref HOURS_UDF: ScalarUDF =
        make_datepart_udf(extract_hour, true, "hours");
    pub static ref MINUTES_UDF: ScalarUDF =
        make_datepart_udf(extract_minute, true, "minutes");
    pub static ref SECONDS_UDF: ScalarUDF =
        make_datepart_udf(extract_second, true, "seconds");
    pub static ref MILLISECONDS_UDF: ScalarUDF =
        make_datepart_udf(extract_millisecond, true, "milliseconds");

    // UTC
    pub static ref UTCYEAR_UDF: ScalarUDF =
        make_datepart_udf(extract_year, false, "utcyear");
    pub static ref UTCMONTH_UDF: ScalarUDF =
        make_datepart_udf(extract_month, false, "utcmonth");
    pub static ref UTCQUARTER_UDF: ScalarUDF =
        make_datepart_udf(extract_quarter, false, "utcquarter");
    pub static ref UTCDATE_UDF: ScalarUDF =
        make_datepart_udf(extract_date, false, "utcdate");
    pub static ref UTCDAYOFYEAR_UDF: ScalarUDF =
        make_datepart_udf(extract_dayofyear, false, "utcdayofyear");
    pub static ref UTCDAY_UDF: ScalarUDF =
        make_datepart_udf(extract_day, false, "utcday");
    pub static ref UTCHOURS_UDF: ScalarUDF =
        make_datepart_udf(extract_hour, false, "utchours");
    pub static ref UTCMINUTES_UDF: ScalarUDF =
        make_datepart_udf(extract_minute, false, "utcminutes");
    pub static ref UTCSECONDS_UDF: ScalarUDF =
        make_datepart_udf(extract_second, false, "utcseconds");
    pub static ref UTCMILLISECONDS_UDF: ScalarUDF =
        make_datepart_udf(extract_millisecond, false, "utcmilliseconds");
}
