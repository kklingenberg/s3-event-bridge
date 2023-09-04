//! Provides a wrapper around jaq to operate on JSON values with jq
//! filters.

use anyhow::{anyhow, Result};
use itertools::Itertools;
pub use jaq_interpret::Filter;
use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};
use serde_json::Value;
use tracing::warn;

/// Compile a filter.
pub fn compile(filter: &str) -> Result<Filter> {
    let mut defs = ParseCtx::new(Vec::new());
    defs.insert_natives(jaq_core::core());
    defs.insert_defs(jaq_std::std());
    let (f, errs) = jaq_parse::parse(filter, jaq_parse::main());
    if !errs.is_empty() {
        return Err(anyhow!(errs.into_iter().join("; ")));
    }
    let f = defs.compile(f.unwrap());
    if !defs.errs.is_empty() {
        return Err(anyhow!(defs.errs.into_iter().map(|(e, _)| e).join("; ")));
    }
    Ok(f)
}

/// Execute a compiled filter against an input, and produce the first
/// serde_json value.
pub fn first_result(filter: &Filter, input: Value) -> Option<Result<Value>> {
    let inputs = RcIter::new(core::iter::empty());
    let mut outputs = filter
        .run((Ctx::new([], &inputs), Val::from(input)))
        .map(|r| r.map(Value::from).map_err(|e| anyhow!(e.to_string())));
    let first_result = outputs.next();
    if outputs.next().is_some() {
        warn!("Filter returned more than one result; subsequent results are ignored");
    }
    first_result
}
