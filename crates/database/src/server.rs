//! The driver half of the protocol.
//!
//! Every driver reads the same framing, dispatches the same nine names and
//! reports errors the same way, so that lives here rather than three times over.
//! A driver implements [`Driver`] and calls [`serve`]; nothing else about the
//! wire is its business.
//!
//! Deliberately free of gpui and of any async runtime. Concurrency here is
//! plain threads, because the one thing this loop must never do is read a
//! request only after the previous one finished: `cancel` arrives *during* the
//! query it cancels, so a sequential loop would make it unreachable -- the
//! method would exist and never once do anything.

use crate::protocol::{ErrorCode, MAX_CHANGES_PER_CALL, Request, Response, ResponseError};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::sync::mpsc;

/// What a driver must answer.
///
/// One method per wire name, so a driver cannot silently omit one -- a missing
/// arm is a compile error rather than a click that does nothing.
///
/// `&self`, not `&mut self`: [`serve`] runs requests concurrently, so a driver
/// keeps its connections behind its own lock. That is also what makes `cancel`
/// possible at all -- it has to reach a query that is still running.
pub trait Driver: Send + Sync + 'static {
    fn initialize(&self, params: Value) -> Result<Value, ResponseError>;
    fn connect(&self, params: Value) -> Result<Value, ResponseError>;
    fn disconnect(&self, params: Value) -> Result<Value, ResponseError>;
    fn list_schemas(&self, params: Value) -> Result<Value, ResponseError>;
    fn list_tables(&self, params: Value) -> Result<Value, ResponseError>;
    fn describe_table(&self, params: Value) -> Result<Value, ResponseError>;
    fn query(&self, params: Value) -> Result<Value, ResponseError>;
    fn cancel(&self, params: Value) -> Result<Value, ResponseError>;

    /// The one write. Optional: a driver that does not override it answers
    /// `unsupported`, which is also what it reports as `row_updates: false`,
    /// so a driver written before this method existed stays read-only.
    fn update_rows(&self, _params: Value) -> Result<Value, ResponseError> {
        Err(ResponseError::new(
            ErrorCode::Unsupported,
            "this driver does not update rows",
        ))
    }
}

/// Decodes params, runs `body`, encodes the result.
///
/// Saves every driver method from repeating the same two `serde_json` calls and
/// the same "bad params" error, and keeps that error's wording identical across
/// drivers.
pub fn typed<P, R>(
    params: Value,
    body: impl FnOnce(P) -> Result<R, ResponseError>,
) -> Result<Value, ResponseError>
where
    P: DeserializeOwned,
    R: Serialize,
{
    let params = serde_json::from_value(params).map_err(|error| {
        ResponseError::new(ErrorCode::Internal, "the request's params did not decode")
            .with_detail(error.to_string())
    })?;
    let result = body(params)?;
    serde_json::to_value(result).map_err(|error| {
        ResponseError::new(ErrorCode::Internal, "the answer did not encode")
            .with_detail(error.to_string())
    })
}

/// Reads requests until the pipe closes, answering each on its own thread.
///
/// `output` must be the process's stdout and **nothing else may write to it** --
/// one stray line breaks the framing for every message after it. Drivers log to
/// stderr, which the client drains into Zode's log.
///
/// Answers may come back in any order; that is what request ids are for. One
/// writer owns the pipe so two answers can never interleave mid-line.
pub fn serve(
    driver: Arc<impl Driver>,
    input: impl BufRead,
    output: impl Write + Send + 'static,
) -> std::io::Result<()> {
    let (answers, pending) = mpsc::channel::<Response>();
    let writer = std::thread::spawn(move || write_answers(pending, output));

    let mut workers = Vec::new();
    for line in input.lines() {
        let line = line?;
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            // Nothing to answer *to*: without a decodable envelope there is no
            // id to put on a reply, and inventing one would be answering a
            // request nobody made. Say so on stderr and read the next line.
            Err(error) => {
                eprintln!("undecodable request: {error}");
                continue;
            }
        };

        let driver = driver.clone();
        let answers = answers.clone();
        workers.push(std::thread::spawn(move || {
            let id = request.id.clone();
            let response = match dispatch(driver.as_ref(), &request) {
                Ok(result) => Response::ok(id, result),
                Err(error) => Response::err(id, error),
            };
            // The receiver is gone only once the writer has stopped, which
            // means the pipe is closed and this answer has nowhere to go.
            answers.send(response).ok();
        }));
        workers.retain(|worker| !worker.is_finished());
    }

    // Dropping the last sender is what tells the writer to stop; the workers
    // still hold clones, so join them first.
    for worker in workers {
        worker.join().ok();
    }
    drop(answers);
    writer.join().ok();
    Ok(())
}

fn write_answers(pending: mpsc::Receiver<Response>, mut output: impl Write) {
    for response in pending {
        let id = response.id.clone();
        if let Err(error) = write_one(&mut output, &response) {
            // The pipe is gone, so this cannot be reported over it. Stderr is
            // the only channel left.
            eprintln!("could not write the answer to {id:?}: {error}");
            return;
        }
    }
}

fn write_one(output: &mut impl Write, response: &Response) -> std::io::Result<()> {
    serde_json::to_writer(&mut *output, response)?;
    output.write_all(b"\n")?;
    output.flush()
}

/// Looks at the raw params rather than decoding them: the count is all it
/// needs, and a driver that decodes them again should not pay for a batch it is
/// about to be refused.
fn refuse_oversized_batch(params: &Value) -> Result<(), ResponseError> {
    let count = params
        .get("changes")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if count > MAX_CHANGES_PER_CALL {
        return Err(
            ResponseError::new(ErrorCode::InvalidValue, "too many changes in one save").with_detail(
                format!("{count} changes were sent and at most {MAX_CHANGES_PER_CALL} are taken in one call"),
            ),
        );
    }
    Ok(())
}

fn dispatch(driver: &impl Driver, request: &Request) -> Result<Value, ResponseError> {
    let params = request.params.clone();
    match request.method.as_str() {
        "initialize" => driver.initialize(params),
        "connect" => driver.connect(params),
        "disconnect" => driver.disconnect(params),
        "list_schemas" => driver.list_schemas(params),
        "list_tables" => driver.list_tables(params),
        "describe_table" => driver.describe_table(params),
        "query" => driver.query(params),
        "cancel" => driver.cancel(params),
        "update_rows" => {
            refuse_oversized_batch(&params)?;
            driver.update_rows(params)
        }
        unknown => Err(ResponseError::new(
            ErrorCode::Unsupported,
            format!("no such method `{unknown}`"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Request;
    use crate::protocol::RequestId;

    /// Everything a driver is obliged to write, and nothing else: the shape of
    /// a driver written before `update_rows` existed.
    struct OldDriver;

    impl Driver for OldDriver {
        fn initialize(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(Value::Null)
        }
        fn connect(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(Value::Null)
        }
        fn disconnect(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(Value::Null)
        }
        fn list_schemas(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(Value::Null)
        }
        fn list_tables(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(Value::Null)
        }
        fn describe_table(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(Value::Null)
        }
        fn query(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(Value::Null)
        }
        fn cancel(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(Value::Null)
        }
    }

    #[test]
    fn a_driver_that_does_not_override_update_rows_answers_unsupported() {
        let request = Request::new(
            RequestId::Number(1),
            "update_rows",
            serde_json::json!({
                "connection_id": "c", "schema": "s", "table": "t", "changes": []
            }),
        );
        let error = dispatch(&OldDriver, &request).expect_err("the default must not write");
        assert_eq!(error.code, ErrorCode::Unsupported);
    }

    #[test]
    fn every_listed_method_has_a_dispatch_arm() {
        for name in crate::protocol::METHOD_NAMES {
            let request = Request::new(RequestId::Number(1), name, Value::Null);
            let outcome = dispatch(&OldDriver, &request);
            let fell_through = matches!(
                &outcome,
                Err(error) if error.message.starts_with("no such method")
            );
            assert!(!fell_through, "`{name}` is listed but not dispatched");
        }
    }

    /// A driver that takes an `update_rows` of any size, to show the limit is
    /// not the driver's to enforce.
    struct Accepting;

    impl Driver for Accepting {
        fn initialize(&self, params: Value) -> Result<Value, ResponseError> {
            OldDriver.initialize(params)
        }
        fn connect(&self, params: Value) -> Result<Value, ResponseError> {
            OldDriver.connect(params)
        }
        fn disconnect(&self, params: Value) -> Result<Value, ResponseError> {
            OldDriver.disconnect(params)
        }
        fn list_schemas(&self, params: Value) -> Result<Value, ResponseError> {
            OldDriver.list_schemas(params)
        }
        fn list_tables(&self, params: Value) -> Result<Value, ResponseError> {
            OldDriver.list_tables(params)
        }
        fn describe_table(&self, params: Value) -> Result<Value, ResponseError> {
            OldDriver.describe_table(params)
        }
        fn query(&self, params: Value) -> Result<Value, ResponseError> {
            OldDriver.query(params)
        }
        fn cancel(&self, params: Value) -> Result<Value, ResponseError> {
            OldDriver.cancel(params)
        }
        fn update_rows(&self, _: Value) -> Result<Value, ResponseError> {
            Ok(serde_json::json!({ "updated": 0 }))
        }
    }

    fn update_with(changes: usize) -> Request {
        let change = serde_json::json!({
            "key": [{ "column": "id", "value": { "kind": "text", "value": "1" } }],
            "set": [{ "column": "a", "value": { "kind": "null" } }],
        });
        Request::new(
            RequestId::Number(1),
            "update_rows",
            serde_json::json!({
                "connection_id": "c", "schema": "s", "table": "t",
                "changes": vec![change; changes],
            }),
        )
    }

    #[test]
    fn one_call_carries_a_bounded_number_of_changes() {
        let limit = crate::protocol::MAX_CHANGES_PER_CALL;
        dispatch(&Accepting, &update_with(limit)).expect("the limit itself is allowed");
        let error = dispatch(&Accepting, &update_with(limit + 1))
            .expect_err("one past the limit is refused before the driver sees it");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        assert_eq!(error.message, "too many changes in one save");
        assert!(error.data.is_none(), "no single change is at fault");
    }
}
