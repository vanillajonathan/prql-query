use std::io::prelude::*;

use anyhow::{Result, anyhow};
use log::{debug, info, warn, error};

use datafusion::prelude::*;
use datafusion::dataframe::DataFrame;
use datafusion::datasource::listing::{ListingTable, ListingTableConfig};

// writer imports
use datafusion::arrow::{csv, json};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::arrow::util::pretty::pretty_format_batches;
use datafusion::parquet::arrow::arrow_writer;

use crate::{SourcesType, OutputWriter, get_dest_from_to};
use prql_compiler::compile;

pub async fn query(query: &str, sources: &SourcesType, to: &str, database: &str, format: &str, writer: &OutputWriter) -> Result<()> {

    // compile the PRQL to SQL
    let sql = compile(&query)?;
    debug!("sql = {:?}", sql.split_whitespace().collect::<Vec<&str>>().join(" "));

    // Create the context
    let config = SessionConfig::new().with_information_schema(true);
    let ctx = SessionContext::with_config(config);

    for (alias, filename) in sources.iter() {
        if filename.ends_with("csv") {
            ctx.register_csv(alias, filename, CsvReadOptions::new()).await?;
        } else if filename.ends_with("json") {
            ctx.register_json(alias, filename, NdJsonReadOptions::default()).await?;
        } else if filename.ends_with("parquet") {
            ctx.register_parquet(alias, filename, ParquetReadOptions::default()).await?;
        } else {
            unimplemented!("filename={filename:?}");
        }
    }

    // Run the query
    let df = ctx.sql(&sql).await?;
    //let rbs = df.collect().await?;

    match writer {
        OutputWriter::arrow => write_results_with_arrow(&df.collect().await?, to, format),
        OutputWriter::backend => write_results_with_datafusion(&df, to, format).await
    }
}

async fn write_results_with_datafusion(df: &DataFrame, to: &str, format: &str) -> Result<()> {
    // Write the results using the native datafusion writer
    if format == "csv" {
        df.write_csv(to).await?;
    } else if format == "json" {
        df.write_json(to).await?;
    } else if format == "parquet" {
        df.write_parquet(to, None).await?;
    } else if format == "table" {
        df.show().await?;
    }

    Ok(())
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
