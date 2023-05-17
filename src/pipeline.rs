//! Geocoding support.

use anyhow::{format_err, Context, Error};
use csv::{self, StringRecord};
use futures::{executor::block_on, future, FutureExt, StreamExt};
use metrics::{counter, describe_counter};
use std::sync::atomic::AtomicI64;
use std::{
    cmp::max, io, iter::FromIterator, sync::Arc, thread::sleep, time::Duration,
};
use strum_macros::EnumString;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, instrument, trace, warn};

use crate::addresses::AddressColumnSpec;
use crate::async_util::run_sync_fn_in_background;
use crate::errors::display_causes_and_backtrace;
use crate::geocoders::Geocoder;
use crate::Result;

/// The number of chunks to buffer on our internal channels.
const CHANNEL_BUFFER: usize = 8;

/// The number of concurrent workers to run.
pub const CONCURRENCY: usize = 48;

/// The number of addresses to pass to our geocoder at one time.
pub const GEOCODE_SIZE: usize = 72;

/// This is the maximum number of chunks that we expect to see in our pipeline
/// at any one time. If we see more than this, it means that backpressure isn't
/// working correctly somewhere.
///
/// Here's how we compute this:
///
/// - We have up to `CONCURRENCY` workers, each of which can process a chunk.
/// - We have two channels, one between the CSV reader and the workers, and one
///   between the workers and the CSV writer. Each of these channels can buffer
///   up to `CHANNEL_BUFFER` chunks.
/// - We may have one chunk in the CSV reader and one chunk in the CSV writer.
/// - We allow up to 10 chunks just in case we're overlooking something
///   in the async machinery that allows a few extra chunks.
const MAX_EXPECTED_CHUNKS: usize = CHANNEL_BUFFER * 2 + CONCURRENCY + 2 + 10;

/// What should we do if a geocoding output column has the same as a column in
/// the input?
#[derive(Debug, Clone, Copy, EnumString, Eq, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum OnDuplicateColumns {
    /// Fail with an error.
    Error,
    /// Replace existing columns with the same name.
    Replace,
    /// Leave the old columns in place and append the new ones.
    Append,
}

/// Data about the CSV file that we include with every chunk to be geocoded.
struct Shared {
    /// Which columns contain addresses that we need to geocode?
    spec: AddressColumnSpec<usize>,
    /// The header of the output CSV file.
    out_headers: StringRecord,
}

/// We use an atomic counter to keep track of how many chunks currently exist.
/// This is used to make sure that we're not seeing parts of our pipeline that
/// are allowing too much
/// ["backpressure"](https://ferd.ca/queues-don-t-fix-overload.html) to build
/// up.
static TOTAL_CHUNKS_EXISTING: AtomicI64 = AtomicI64::new(0);

/// A chunk to geocode.
struct Chunk {
    /// Shared information about the CSV file, including headers.
    shared: Arc<Shared>,
    /// The rows to geocode.
    rows: Vec<StringRecord>,
}

impl Chunk {
    /// Create a new `Chunk`.
    fn new(shared: Arc<Shared>, rows: Vec<StringRecord>) -> Chunk {
        let existing =
            TOTAL_CHUNKS_EXISTING.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if existing > MAX_EXPECTED_CHUNKS as i64 {
            panic!(
                "too many chunks in the pipeline: found {}, expected at most {}",
                existing, MAX_EXPECTED_CHUNKS
            );
        }
        Chunk { shared, rows }
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        let existing =
            TOTAL_CHUNKS_EXISTING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        if existing < 0 {
            panic!(
                "we apparently have negative chunks ({}) in the pipeline?",
                existing
            );
        }
    }
}

/// A message sent on our channel.
enum Message {
    /// A chunk to geocode.
    Chunk(Chunk),

    /// The end of our stream. Sent when all data has been processed
    /// successfuly.
    EndOfStream,
}

/// Read CSVs from standard input, geocode them, and write them to standard
/// output.
pub async fn geocode_stdio(
    spec: AddressColumnSpec<String>,
    geocoder: Arc<dyn Geocoder>,
    on_duplicate_columns: OnDuplicateColumns,
    max_retries: u8,
) -> Result<()> {
    describe_counter!("geocodecsv.addresses.total", "Total addresses processed");
    describe_counter!("geocodecsv.chunks.total", "Total address chunks processed");
    describe_counter!(
        "geocodecsv.chunks_retried.total",
        "Total address chunks retried"
    );
    describe_counter!(
        "geocodecsv.chunks_failed.total",
        "total address chunks that failed after all retries"
    );

    // Set up bounded channels for communication between the sync and async
    // worlds.
    let (in_tx, in_rx) = mpsc::channel::<Message>(CHANNEL_BUFFER);
    let (out_tx, out_rx) = mpsc::channel::<Message>(CHANNEL_BUFFER);

    // Hook up our inputs and outputs, which are synchronous functions running
    // in their own threads.
    let geocoder2 = geocoder.clone();
    let read_fut = run_sync_fn_in_background("read CSV".to_owned(), move || {
        read_csv_from_stdin(spec, geocoder2.as_ref(), on_duplicate_columns, in_tx)
    });
    let write_fut = run_sync_fn_in_background("write CSV".to_owned(), move || {
        write_csv_to_stdout(out_rx)
    });

    // Geocode each chunk that we see, with up to `CONCURRENCY` chunks being
    // geocoded at a time.
    let geocode_fut = async move {
        let geocoder = geocoder.clone();
        let in_rx = ReceiverStream::new(in_rx);
        let mut stream = in_rx
            // Turn input messages into futures that yield output messages.
            .map(move |message| {
                geocode_message(geocoder.clone(), message, max_retries).boxed()
            })
            // Turn output message futures into output messages in parallel.
            .buffered(CONCURRENCY);

        // Forward our results to our output.
        while let Some(result) = stream.next().await {
            out_tx
                .send(result?)
                .await
                .map_err(|_| format_err!("could not send message to output thread"))?;
        }

        Ok::<_, Error>(())
    }
    .boxed();

    // Wait for all three of our processes to finish.
    let (read_result, geocode_result, write_result) =
        future::join3(read_fut, geocode_fut, write_fut).await;

    // Wrap any errors with context.
    let read_result: Result<()> = read_result.context("error reading input");
    let geocode_result: Result<()> = geocode_result.context("error geocoding");
    let write_result: Result<()> = write_result.context("error writing output");

    // Print if one of the processes fails, it will usually cause the other two
    // to fail. We could try to figure out the "root" cause for the user, or we
    // could just print out all the errors and let the user sort them out. :-(
    let mut failed = false;
    if let Err(err) = &read_result {
        failed = true;
        display_causes_and_backtrace(err);
    }
    if let Err(err) = &geocode_result {
        failed = true;
        display_causes_and_backtrace(err);
    }
    if let Err(err) = &write_result {
        failed = true;
        display_causes_and_backtrace(err);
    }

    if failed {
        Err(format_err!(
            "geocoding stdio failed because of the above errors"
        ))
    } else {
        Ok(())
    }
}

/// Read a CSV file and write it as messages to `tx`.
fn read_csv_from_stdin(
    spec: AddressColumnSpec<String>,
    geocoder: &dyn Geocoder,
    on_duplicate_columns: OnDuplicateColumns,
    tx: Sender<Message>,
) -> Result<()> {
    // Open up our CSV file and get the headers.
    let stdin = io::stdin();
    let mut rdr = csv::Reader::from_reader(stdin.lock());
    let mut in_headers = rdr.headers()?.to_owned();
    debug!("input headers: {:?}", in_headers);

    // Figure out if we have any duplicate columns.
    let (duplicate_column_indices, duplicate_column_names) = {
        let duplicate_columns = spec.duplicate_columns(geocoder, &in_headers)?;
        let indices = duplicate_columns
            .iter()
            .map(|name_idx| name_idx.1)
            .collect::<Vec<_>>();
        let names = duplicate_columns
            .iter()
            .map(|name_idx| name_idx.0)
            .collect::<Vec<_>>()
            .join(", ");
        (indices, names)
    };

    // If we do have duplicate columns, figure out what to do about it.
    let mut should_remove_columns = false;
    let mut remove_column_flags = vec![false; in_headers.len()];
    if !duplicate_column_indices.is_empty() {
        match on_duplicate_columns {
            OnDuplicateColumns::Error => {
                return Err(format_err!(
                    "input columns would conflict with geocoding columns: {}",
                    duplicate_column_names,
                ));
            }
            OnDuplicateColumns::Replace => {
                warn!("replacing input columns: {}", duplicate_column_names);
                should_remove_columns = true;
                for i in duplicate_column_indices.iter().cloned() {
                    remove_column_flags[i] = true;
                }
            }
            OnDuplicateColumns::Append => {
                warn!(
                    "output contains duplicate columns: {}",
                    duplicate_column_names,
                );
            }
        }
    }

    // Remove any duplicate columns from our input headers.
    if should_remove_columns {
        in_headers = remove_columns(&in_headers, &remove_column_flags);
    }

    // Convert our column spec from using header names to header indices.
    //
    // This needs to happen _after_ `remove_columns` on our headers!
    let spec = spec.convert_to_indices_using_headers(&in_headers)?;

    // Decide how big to make our chunks. We want to geocode no more
    // `GEOCODE`-size addresses at a time, and each input row may generate up to
    // `spec.prefix_count()` addresses.
    let chunk_size = max(1, GEOCODE_SIZE / max(spec.prefix_count(), 1));
    assert!(chunk_size > 0 && chunk_size <= GEOCODE_SIZE);

    // Build our output headers.
    let mut out_headers = in_headers;
    for prefix in spec.prefixes() {
        geocoder.add_header_columns(prefix, &mut out_headers);
    }
    debug!("output headers: {:?}", out_headers);

    // Build our shared CSV file metadata, and wrap it with a reference count.
    let shared = Arc::new(Shared { spec, out_headers });

    // Group up the rows into chunks and send them to `tx`.
    let mut sent_chunk = false;
    let mut rows = Vec::with_capacity(chunk_size);
    for row in rdr.records() {
        let mut row = row?;
        if should_remove_columns {
            // Strip out any duplicate columns.
            row = remove_columns(&row, &remove_column_flags);
        }
        rows.push(row);
        if rows.len() >= chunk_size {
            trace!("sending {} input rows", rows.len());
            block_on(tx.send(Message::Chunk(Chunk::new(shared.clone(), rows))))
                .map_err(|_| {
                    format_err!("could not send rows to geocoder (perhaps it failed)")
                })?;
            sent_chunk = true;
            rows = Vec::with_capacity(chunk_size);
        }
    }

    // Send a final chunk if either (1) we never sent a chunk, or (2) we have
    // rows that haven't been sent yet.
    if !sent_chunk || !rows.is_empty() {
        trace!("sending final {} input rows", rows.len());
        block_on(tx.send(Message::Chunk(Chunk { shared, rows }))).map_err(|_| {
            format_err!("could not send rows to geocoder (perhaps it failed)")
        })?;
    }

    // Confirm that we've seen the end of the stream.
    trace!("sending end-of-stream for input");
    block_on(tx.send(Message::EndOfStream)).map_err(|_| {
        format_err!("could not send end-of-stream to geocoder (perhaps it failed)")
    })?;

    debug!("done sending input");
    Ok(())
}

/// Remove columns from `row` if they're set to true in `remove_column_flags`.
fn remove_columns(row: &StringRecord, remove_column_flags: &[bool]) -> StringRecord {
    debug_assert_eq!(row.len(), remove_column_flags.len());
    StringRecord::from_iter(row.iter().zip(remove_column_flags).filter_map(
        |(value, &remove)| {
            if remove {
                None
            } else {
                Some(value.to_owned())
            }
        },
    ))
}

/// Receive chunks of a CSV file from `rx` and write them to standard output.
fn write_csv_to_stdout(rx: Receiver<Message>) -> Result<()> {
    let stdout = io::stdout();
    let mut wtr = csv::Writer::from_writer(stdout.lock());

    let mut headers_written = false;
    let mut end_of_stream_seen = false;
    let mut rx = ReceiverStream::new(rx);
    while let Some(message) = block_on(rx.next()) {
        match message {
            Message::Chunk(chunk) => {
                trace!("received {} output rows", chunk.rows.len());
                if !headers_written {
                    wtr.write_record(&chunk.shared.out_headers)?;
                    headers_written = true;
                }
                for row in &chunk.rows {
                    wtr.write_record(row)?;
                }
            }
            Message::EndOfStream => {
                trace!("received end-of-stream for output");
                assert!(headers_written);
                end_of_stream_seen = true;
                break;
            }
        }
    }
    if !end_of_stream_seen {
        // The background thread exitted without sending anything. This
        // shouldn't happen.
        error!("did not receive end-of-stream");
        return Err(format_err!(
            "did not receive end-of-stream from geocoder (perhaps it failed)"
        ));
    }
    Ok(())
}

/// Geocode a `Message`. This is just a wrapper around `geocode_chunk`.
async fn geocode_message(
    geocoder: Arc<dyn Geocoder>,
    message: Message,
    max_retries: u8,
) -> Result<Message> {
    match message {
        Message::Chunk(chunk) => {
            trace!("geocoding {} rows", chunk.rows.len());
            Ok(Message::Chunk(
                geocode_chunk(geocoder.as_ref(), chunk, max_retries).await?,
            ))
        }
        Message::EndOfStream => {
            trace!("geocoding received end-of-stream");
            Ok(Message::EndOfStream)
        }
    }
}

/// Geocode a `Chunk`.
#[instrument(
    level="debug",
    skip_all,
    fields(rows = chunk.rows.len())
)]
async fn geocode_chunk(
    geocoder: &dyn Geocoder,
    mut chunk: Chunk,
    max_retries: u8,
) -> Result<Chunk> {
    // Build a list of addresses to geocode.
    let prefixes = chunk.shared.spec.prefixes();
    let mut addresses = vec![];
    for prefix in &prefixes {
        let column_keys = chunk
            .shared
            .spec
            .get(prefix)
            .expect("should always have prefix");
        for row in &chunk.rows {
            addresses.push(column_keys.extract_address_from_record(row)?);
        }
    }
    let addresses_len = addresses.len();

    // Geocode our addresses.
    trace!("geocoding {} addresses", addresses_len);
    let mut failures: u8 = 0;
    let mut retry_wait = Duration::from_secs(2);
    let geocoded = loop {
        // TODO: The `clone` here is expensive. We might want to move the
        // `retry` loop inside of `street_addresses`.
        let result = geocoder.geocode_addresses(&addresses).await;
        match result {
            Err(ref err) if failures < max_retries => {
                failures += 1;
                debug!(
                    "retrying geocoder error (waiting {} secs): {:?}",
                    retry_wait.as_secs(),
                    err
                );
                counter!("geocodecsv.chunks_retried.total", 1);
                sleep(retry_wait);
                retry_wait *= 2;
            }
            Err(err) => {
                counter!("geocodecsv.chunks_failed.total", 1);
                return Err(err).context("geocoder error");
            }
            Ok(geocoded) => {
                counter!("geocodecsv.chunks.total", 1);
                break geocoded;
            }
        }
    };
    counter!("geocodecsv.addresses.total", addresses_len as u64);
    trace!("geocoded {} addresses", addresses_len);

    // Add address information to our output rows.
    for geocoded_for_prefix in geocoded.chunks(chunk.rows.len()) {
        assert_eq!(geocoded_for_prefix.len(), chunk.rows.len());
        for (response, row) in geocoded_for_prefix.iter().zip(&mut chunk.rows) {
            if let Some(response) = response {
                geocoder.add_value_columns_to_row(response, row);
            } else {
                geocoder.add_empty_columns_to_row(row);
            }
        }
    }
    Ok(chunk)
}
