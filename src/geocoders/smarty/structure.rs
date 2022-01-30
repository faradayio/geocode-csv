//! Various subsets of data potenitally returned by Smarty.
//!
//! There used to be an idea that we'd allow users to customize this, but it
//! never happened, and it probably never will, now that we're moving to
//! multiple backends. So this is now an implementation detail.

use anyhow::format_err;
use serde_json::{self, Map, Value};
use std::borrow::Cow;

use crate::Result;

/// A subset of the fields returned by Smarty.
#[derive(Debug)]
pub struct Structure {
    /// Number of columns added.
    column_count: usize,

    /// Fields that we want to include.
    ///
    /// WARNING: The correctness of the `traverse` function depends on `Map`
    /// being an order-preserving map type, which is set using `preserve_order`
    /// in `Cargo.toml`.
    fields: Map<String, Value>,
}

/// All the fields that we normally care about.
const COMPLETE: &str = include_str!("structures/complete.json");

impl Structure {
    /// A `Structure` including all the fields we normally care about.
    pub fn complete() -> Result<Structure> {
        Self::from_str(COMPLETE)
    }

    /// Parse a `Structure` from a string containing JSON.
    fn from_str(s: &str) -> Result<Structure> {
        // Parse our JSON and build our structure.
        let fields = serde_json::from_str(s)?;
        let mut structure = Structure {
            column_count: 0,
            fields,
        };

        // Update our column count.
        let mut count = 0;
        structure.traverse(|_path| {
            count += 1;
            Ok(())
        })?;
        structure.column_count = count;
        Ok(structure)
    }

    /// Given the path to a colum in our [`structure::Structure`], return the
    /// column name we should use. This will panic if `path` is empty, because
    /// that should be impossible.
    fn column_name(&self, path: &[&str]) -> String {
        let last = path
            .last()
            .expect("should always have at least one path element");
        last.to_string()
    }

    /// Return all the columns that this structure will add to a CSV file.
    pub fn output_column_names(&self) -> Result<Vec<String>> {
        let mut columns = vec![];
        self.traverse(|path| {
            let name = self.column_name(path);
            columns.push(name);
            Ok(())
        })?;
        Ok(columns)
    }

    /// Extract fields from `data` and merge them into `row`.
    ///
    /// PERFORMANCE: This is probably slower than it should be in a hot loop.
    pub fn value_columns_for(&self, data: &Value) -> Result<Vec<String>> {
        let mut result = vec![];
        self.traverse(|path| {
            // Follow `path`.
            let mut focus = data;
            for key in path {
                if let Some(value) = focus.get(key) {
                    focus = value;
                } else {
                    // No value present, so push an empty field.
                    result.push("".to_owned());
                    return Ok(());
                }
            }

            // Add the value to our row.
            let formatted = match focus {
                Value::Bool(b) => Cow::Borrowed(if *b { "T" } else { "F" }),
                Value::Null => Cow::Borrowed(""),
                Value::Number(n) => Cow::Owned(format!("{}", n)),
                Value::String(s) => Cow::Borrowed(&s[..]),
                Value::Array(_) | Value::Object(_) => {
                    return Err(format_err!(
                        "unexpected value at {:?}: {:?}",
                        path,
                        focus
                    ));
                }
            };
            result.push(formatted.into_owned());
            Ok(())
        })?;
        Ok(result)
    }

    /// Generic Smarty result traverser. Calls `f` with the path to
    /// each key present in this `Structure`.
    fn traverse<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(&[&str]) -> Result<()>,
    {
        let mut path = Vec::with_capacity(2);
        for (key, value) in &self.fields {
            path.push(&key[..]);
            match value {
                Value::Bool(true) => f(&path)?,
                Value::Bool(false) => {}
                Value::Object(map) => {
                    for (key, value) in map {
                        path.push(&key[..]);
                        match value {
                            Value::Bool(true) => f(&path)?,
                            Value::Bool(false) => {}
                            _ => {
                                return Err(format_err!(
                                    "invalid structure at {:?}: {:?}",
                                    path,
                                    value,
                                ));
                            }
                        }
                        path.pop();
                    }
                }
                _ => {
                    return Err(format_err!(
                        "invalid structure at {:?}: {:?}",
                        path,
                        value,
                    ));
                }
            }
            path.pop();
        }
        Ok(())
    }
}

#[test]
fn output_column_names() {
    let structure = Structure::complete().unwrap();
    let column_names = structure.output_column_names().unwrap();
    let expected = &[
        "addressee",
        "delivery_line_1",
        "delivery_line_2",
        "last_line",
        "delivery_point_barcode",
        "urbanization",
        "primary_number",
        "street_name",
        "street_predirection",
        "street_postdirection",
        "street_suffix",
        "secondary_number",
        "secondary_designator",
        "extra_secondary_number",
        "extra_secondary_designator",
        "pmb_designator",
        "pmb_number",
        "city_name",
        "default_city_name",
        "state_abbreviation",
        "zipcode",
        "plus4_code",
        "delivery_point",
        "delivery_point_check_digit",
        "record_type",
        "zip_type",
        "county_fips",
        "county_name",
        "carrier_route",
        "congressional_district",
        "building_default_indicator",
        "rdi",
        "elot_sequence",
        "elot_sort",
        "latitude",
        "longitude",
        "precision",
        "time_zone",
        "utc_offset",
        "dst",
        "dpv_match_code",
        "dpv_footnotes",
        "dpv_cmra",
        "dpv_vacant",
        "active",
        "ews_match",
        "footnotes",
        "lacslink_code",
        "lacslink_indicator",
        "suitelink_match",
    ][..];
    assert_eq!(column_names, expected);
}

#[test]
fn value_columns_for() {
    let structure = Structure::complete().unwrap();

    let data: Value = serde_json::from_str(
        r#"{
    "addressee": "ACME, Inc.",
    "metadata": {
        "precision": "Zip5"
    }
}"#,
    )
    .unwrap();

    let row = structure.value_columns_for(&data).unwrap();
    let expected = &[
        "ACME, Inc.",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "Zip5",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
    ][..];
    assert_eq!(row, expected);
}
