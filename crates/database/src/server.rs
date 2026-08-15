//! The driver half of the protocol.
//!
//! Every driver reads the same framing, dispatches the same eight names and
//! reports errors the same way, so that lives here rather than three times over.
//! A driver implements [`Driver`] and calls [`serve`]; nothing else about the
//! wire is its business.
//!
//! Deliberately free of gpui and of any async runtime. Concurrency here is
//! plain threads, because the one thing this loop must never do is read a
//! request only after the previous one finished: `cancel` arrives *during* the
//! query it cancels, so a sequential loop would make it unreachable -- the
//! method would exist and never once do anything.

use crate::protocol::{ErrorCode, Request, Response, ResponseError};
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
        unknown => Err(ResponseError::new(
            ErrorCode::Unsupported,
            format!("no such method `{unknown}`"),
        )),
    }
}
