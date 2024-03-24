//! Paired geocoder. Run two geocoders, return both.
//!
//! ¿Por qué no los dos?

use async_trait::async_trait;

use crate::format_err;
use crate::geocoders::{Geocoded, Geocoder};
use crate::{addresses::Address, Result};

/// A geocoder that runs two geocoders and returns both results.
pub struct Paired {
    /// The first geocoder.
    fst: Box<dyn Geocoder>,

    /// The second geocoder.
    snd: Box<dyn Geocoder>,

    /// An empty set of columns with the same width as the first geocoder.
    fst_empty_output: Geocoded,

    /// An empty set of columns with the same width as the second geocoder.
    snd_empty_output: Geocoded,

    /// The column names output by this geocoder. Includes both sets
    /// of column names.
    column_names: Vec<String>,

    /// The configuration key for this geocoder.
    config_key: String,
}

impl Paired {
    /// Create a new geocoder which returns the results of two geocoders. The
    /// columns from the first geocoder are returned first, followed by the
    /// columns from the second geocoder. The column names from the second
    /// geocoder are prefixed with the tag of the second geocoder.
    pub fn new(
        fst: Box<dyn Geocoder>,
        snd_label: &str,
        snd: Box<dyn Geocoder>,
    ) -> Paired {
        let fst_column_names = fst.column_names().iter().cloned();
        let snd_column_names = snd
            .column_names()
            .iter()
            .map(|c| format!("{}_{}", snd_label, c));
        let fst_empty_output = Geocoded {
            column_values: vec!["".to_owned(); fst.column_names().len()],
        };
        let snd_empty_output = Geocoded {
            column_values: vec!["".to_owned(); snd.column_names().len()],
        };
        let column_names = fst_column_names.chain(snd_column_names).collect();
        let config_key =
            format!("{}+{}", fst.configuration_key(), snd.configuration_key());
        Paired {
            fst,
            snd,
            fst_empty_output,
            snd_empty_output,
            column_names,
            config_key,
        }
    }

    fn combine_geocoder_results(
        &self,
        fst: Option<Geocoded>,
        snd: Option<Geocoded>,
    ) -> Option<Geocoded> {
        match (fst, snd) {
            (None, None) => None,
            (Some(f), None) => Some(f.concat(&self.snd_empty_output)),
            (None, Some(s)) => Some(self.fst_empty_output.concat(&s)),
            (Some(f), Some(s)) => Some(f.concat(&s)),
        }
    }
}

#[async_trait]
impl Geocoder for Paired {
    fn tag(&self) -> &str {
        "pair"
    }

    fn configuration_key(&self) -> &str {
        &self.config_key
    }

    fn column_names(&self) -> &[String] {
        &self.column_names
    }

    async fn geocode_addresses(
        &self,
        addresses: &[Address],
    ) -> Result<Vec<Option<Geocoded>>> {
        let fst = self.fst.geocode_addresses(addresses).await?;
        let snd = self.snd.geocode_addresses(addresses).await?;
        if fst.len() != snd.len() {
            return Err(format_err!(
                "Geocoders returned different numbers of results: {} from {} vs {} from {}",
                fst.len(),
                self.fst.tag(),
                snd.len(),
                self.snd.tag(),
            ));
        }
        Ok(fst
            .into_iter()
            .zip(snd)
            .map(|(f, s)| self.combine_geocoder_results(f, s))
            .collect())
    }
}
