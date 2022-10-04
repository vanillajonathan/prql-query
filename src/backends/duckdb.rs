use std::io::prelude::*;

use anyhow::Result;
use log::{debug, info, warn, error};

use arrow::{csv, json};
use arrow::record_batch::RecordBatch;
use arrow::util::pretty::pretty_format_batches;
use parquet::arrow::arrow_writer;

use duckdb::{Connection, types::{ValueRef, FromSql}};
use chrono::{DateTime, Utc};

use crate::{SourcesType, OutputWriter, get_dest_from_to};
use prql_compiler::compile;

pub fn query(query: &str, sources: &SourcesType, to: &str, database: &str, format: &str, writer: &OutputWriter) -> Result<()> {

    // prepend CTEs for the source aliases
    let mut query = query.to_string();
    for (alias, source) in sources.iter() {
        // Needs the _{}_ on the LHS for _{}_.*
        query = format!("table {alias} = (from __{alias}__=__file_{alias}__)\n{query}");
    }
    debug!("query = {query:?}");

    // compile the PRQL to SQL
    let mut sql : String = compile(&query)?;
    debug!("sql = {:?}", sql.split_whitespace().collect::<Vec<&str>>().join(" "));

    // replace the table placeholders again
    for (alias, source) in sources.iter() {
        let placeholder = format!("__file_{}__", &alias);
        let quoted_source = format!(r#""{}""#, &source);
        sql = sql.replace(&placeholder, &quoted_source);
    }
    debug!("sql = {sql:?}");

    // prepare the connection and statement
    let conn = Connection::open_in_memory()?;
    let mut stmt = conn.prepare(&sql)?;

    let rbs = stmt.query_arrow([])?.collect::<Vec<RecordBatch>>();

    match writer {
        OutputWriter::arrow => write_results_with_arrow(&rbs, to, format),
        OutputWriter::backend => write_results_with_duckdb(&rbs, to, format)
    }
}

fn write_results_with_duckdb(rbs: &[RecordBatch], to: &str, format: &str) -> Result<()> {
    unimplemented!("write_results_with_duckdb");
}

fn write_results_with_arrow(rbs: &[RecordBatch], to: &str, format: &str) -> Result<()> {

    let mut dest: Box<dyn Write> = get_dest_from_to(to)?;

    if format == "csv" {
        write_record_batches_to_csv(rbs, &mut dest)?;
    } else if format == "json" {
        write_record_batches_to_json(rbs, &mut dest)?;
    } else if format == "parquet" {
        write_record_batches_to_parquet(rbs, &mut dest)?;
    } else if format == "table" {
        write_record_batches_to_table(rbs, &mut dest)?;
    } else {
        unimplemented!("to");
    }

    Ok(())
}

fn write_record_batches_to_csv(rbs: &[RecordBatch], dest: &mut dyn Write) -> Result<()> {
    {
        let mut writer = csv::Writer::new(dest);
        for rb in rbs {
            writer.write(rb)?;
        }
    }
    Ok(())
}

fn write_record_batches_to_json(rbs: &[RecordBatch], dest: &mut dyn Write) -> Result<()> {
    {
        // let mut writer = json::ArrayWriter::new(&mut buf);
        let mut writer = json::LineDelimitedWriter::new(dest);
        writer.write_batches(&rbs)?;
        writer.finish()?;
    }
    Ok(())
}

fn write_record_batches_to_parquet(rbs: &[RecordBatch], dest: &mut dyn Write) -> Result<()> {
    if rbs.is_empty() {
        return Ok(());
    }

    let schema = rbs[0].schema();
    {
        let mut writer = arrow_writer::ArrowWriter::try_new(dest, schema, None)?;

        for rb in rbs {
            writer.write(rb)?;
        }
        writer.close()?;
    }
    Ok(())
}

fn write_record_batches_to_table(rbs: &[RecordBatch], dest: &mut dyn Write) -> Result<()> {
    dest.write(pretty_format_batches(rbs)?.to_string().as_bytes());
    dest.write(b"\n");
    Ok(())
}
