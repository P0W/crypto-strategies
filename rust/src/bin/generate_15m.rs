//! Utility to generate 15m OHLCV data from 5m data
//! 
//! Resamples 5-minute candles to 15-minute candles by aggregating every 3 bars.

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use csv::{Reader, Writer};
use std::error::Error;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
struct Candle {
    datetime: String,  // Store as string to maintain format
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

fn parse_datetime(s: &str) -> Option<NaiveDateTime> {
    // Try parsing as "YYYY-MM-DD HH:MM:SS"
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
}

fn resample_to_15m(candles_5m: Vec<Candle>) -> Vec<Candle> {
    let mut candles_15m = Vec::new();
    let mut i = 0;

    while i + 2 < candles_5m.len() {
        // Aggregate 3 consecutive 5m candles into 1 15m candle
        let c1 = &candles_5m[i];
        let c2 = &candles_5m[i + 1];
        let c3 = &candles_5m[i + 2];

        // Parse datetimes for verification
        let dt1 = match parse_datetime(&c1.datetime) {
            Some(dt) => dt,
            None => {
                i += 1;
                continue;
            }
        };
        let dt2 = match parse_datetime(&c2.datetime) {
            Some(dt) => dt,
            None => {
                i += 1;
                continue;
            }
        };
        let dt3 = match parse_datetime(&c3.datetime) {
            Some(dt) => dt,
            None => {
                i += 1;
                continue;
            }
        };

        // Verify they are consecutive (5-minute spacing)
        let diff1 = dt2.signed_duration_since(dt1);
        let diff2 = dt3.signed_duration_since(dt2);

        if diff1 == Duration::minutes(5) && diff2 == Duration::minutes(5) {
            let candle_15m = Candle {
                datetime: c1.datetime.clone(), // Use first candle's time
                open: c1.open,
                high: c1.high.max(c2.high).max(c3.high),
                low: c1.low.min(c2.low).min(c3.low),
                close: c3.close,
                volume: c1.volume + c2.volume + c3.volume,
            };
            candles_15m.push(candle_15m);
            i += 3;
        } else {
            // Skip if not consecutive
            i += 1;
        }
    }

    candles_15m
}

fn main() -> Result<(), Box<dyn Error>> {
    let data_dir = Path::new("../data");
    
    // Find all 5m files
    let files_5m: Vec<_> = fs::read_dir(data_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .ends_with("_5m.csv")
        })
        .collect();

    for entry in files_5m {
        let path_5m = entry.path();
        let filename = entry.file_name();
        let filename_str = filename.to_string_lossy();
        
        // Generate output filename (replace _5m with _15m)
        let output_filename = filename_str.replace("_5m.csv", "_15m.csv");
        let output_path = data_dir.join(&output_filename);

        println!("Processing: {} -> {}", filename_str, output_filename);

        // Read 5m data
        let mut rdr = Reader::from_path(&path_5m)?;
        let mut candles_5m = Vec::new();

        // Skip header
        let records = rdr.records();
        
        for result in records {
            let record = result?;
            if record.len() < 6 {
                continue; // Skip invalid records
            }
            
            let datetime_str = record[0].trim().to_string();
            
            // Validate it's parseable
            if parse_datetime(&datetime_str).is_none() {
                continue;
            }
            
            let open = record[1].parse::<f64>()?;
            let high = record[2].parse::<f64>()?;
            let low = record[3].parse::<f64>()?;
            let close = record[4].parse::<f64>()?;
            let volume = record[5].parse::<f64>()?;
            
            candles_5m.push(Candle {
                datetime: datetime_str,
                open,
                high,
                low,
                close,
                volume,
            });
        }

        // Resample to 15m
        let candles_15m = resample_to_15m(candles_5m);
        
        println!("  Generated {} 15m candles", candles_15m.len());

        // Write 15m data
        let mut wtr = Writer::from_path(&output_path)?;
        wtr.write_record(&["datetime", "open", "high", "low", "close", "volume"])?;
        
        for candle in candles_15m {
            wtr.write_record(&[
                &candle.datetime,
                &candle.open.to_string(),
                &candle.high.to_string(),
                &candle.low.to_string(),
                &candle.close.to_string(),
                &candle.volume.to_string(),
            ])?;
        }
        
        wtr.flush()?;
    }

    println!("\n15m data generation complete!");
    Ok(())
}
