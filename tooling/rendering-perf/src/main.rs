//! CPU microbenchmarks, not application frame time or GPU benchmarks.

mod layout;
#[cfg(feature = "gpui-scene")]
mod scene;

use std::{
    env,
    error::Error,
    fs::{self, File},
    hint::black_box,
    io::{BufWriter, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default)]
struct Work {
    leaf_layouts: u64,
    measures: u64,
    primitives: u64,
    // Source-derived payload estimate only. No GPU upload is executed here.
    modeled_instance_bytes: u64,
}

impl std::ops::AddAssign for Work {
    fn add_assign(&mut self, other: Self) {
        self.leaf_layouts += other.leaf_layouts;
        self.measures += other.measures;
        self.primitives += other.primitives;
        self.modeled_instance_bytes += other.modeled_instance_bytes;
    }
}

struct Config {
    output: PathBuf,
    sizes: Vec<usize>,
    samples: usize,
    min_batch: Duration,
}

impl Config {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let mut config = Self {
            output: PathBuf::from("target/rendering-perf"),
            sizes: vec![300, 3_000, 30_000],
            samples: 101,
            min_batch: Duration::from_micros(1_000),
        };
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--help" {
                println!(
                    "rendering-perf [--output DIR] [--sizes 300,3000,30000] \
                     [--samples 101] [--min-batch-us 1000]"
                );
                std::process::exit(0);
            }
            let value = args.next().ok_or("option requires a value")?;
            match arg.as_str() {
                "--output" => config.output = value.into(),
                "--sizes" => {
                    config.sizes = value.split(',').map(str::parse).collect::<Result<_, _>>()?;
                }
                "--samples" => config.samples = value.parse()?,
                "--min-batch-us" => config.min_batch = Duration::from_micros(value.parse()?),
                _ => return Err(format!("unknown option: {arg}").into()),
            }
        }
        if config.sizes.is_empty()
            || config.sizes.iter().any(|n| *n == 0 || *n > 1_000_000)
            || !(10..=10_000).contains(&config.samples)
            || config.min_batch.is_zero()
            || config.min_batch > Duration::from_secs(1)
        {
            return Err("sizes must be 1..=1000000; samples 10..=10000; batch 1us..=1s".into());
        }
        Ok(config)
    }
}

struct Recorder {
    config: Config,
    summary: BufWriter<File>,
    raw: BufWriter<File>,
}

impl Recorder {
    fn new(config: Config) -> Result<Self, Box<dyn Error>> {
        fs::create_dir_all(&config.output)?;
        let mut summary = BufWriter::new(File::create(config.output.join("summary.csv"))?);
        let mut raw = BufWriter::new(File::create(config.output.join("samples.csv"))?);
        writeln!(
            summary,
            "case,nodes,samples,ops_per_sample,p50_ns,p95_ns,p99_ns,leaf_layouts_per_op,measures_per_op,primitives_per_op,modeled_instance_bytes_per_op"
        )?;
        writeln!(
            raw,
            "case,nodes,sample,operations,elapsed_ns,leaf_layouts,measures,primitives,modeled_instance_bytes"
        )?;
        Ok(Self {
            config,
            summary,
            raw,
        })
    }

    fn run(
        &mut self,
        name: &str,
        nodes: usize,
        mut op: impl FnMut() -> Work,
    ) -> Result<(), Box<dyn Error>> {
        for _ in 0..8 {
            black_box(op());
        }
        let mut operations = 1_u64;
        loop {
            let start = Instant::now();
            for _ in 0..operations {
                black_box(op());
            }
            if start.elapsed() >= self.config.min_batch || operations >= 1 << 20 {
                break;
            }
            operations *= 2;
        }
        let mut latencies = Vec::with_capacity(self.config.samples);
        let mut total = Work::default();
        for sample in 0..self.config.samples {
            let mut work = Work::default();
            let start = Instant::now();
            for _ in 0..operations {
                work += black_box(op());
            }
            let elapsed = start.elapsed().as_nanos();
            latencies.push(elapsed as f64 / operations as f64);
            writeln!(
                self.raw,
                "{name},{nodes},{sample},{operations},{elapsed},{},{},{},{}",
                work.leaf_layouts, work.measures, work.primitives, work.modeled_instance_bytes
            )?;
            total += work;
        }
        latencies.sort_by(f64::total_cmp);
        let count = (operations * self.config.samples as u64) as f64;
        let p50 = percentile(&latencies, 50);
        let p95 = percentile(&latencies, 95);
        let p99 = percentile(&latencies, 99);
        writeln!(
            self.summary,
            "{name},{nodes},{},{operations},{p50:.3},{p95:.3},{p99:.3},{:.3},{:.3},{:.3},{:.3}",
            self.config.samples,
            total.leaf_layouts as f64 / count,
            total.measures as f64 / count,
            total.primitives as f64 / count,
            total.modeled_instance_bytes as f64 / count,
        )?;
        self.summary.flush()?;
        self.raw.flush()?;
        println!(
            "{name:30} n={nodes:6} p50={:10.3}us p95={:10.3}us leaf={:8.1} measure={:8.1}",
            p50 / 1_000.0,
            p95 / 1_000.0,
            total.leaf_layouts as f64 / count,
            total.measures as f64 / count,
        );
        Ok(())
    }
}

fn percentile(sorted: &[f64], percent: usize) -> f64 {
    sorted[(sorted.len() * percent).div_ceil(100) - 1]
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut recorder = Recorder::new(Config::parse()?)?;
    println!("CPU microbenchmarks; percentiles describe batched ns/op, not frame latency.");
    for n in recorder.config.sizes.clone() {
        layout::run(&mut recorder, n)?;
        #[cfg(feature = "gpui-scene")]
        scene::run(&mut recorder, n)?;
    }
    println!("Results: {}", recorder.config.output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn percentile_uses_nearest_rank_including_tail() {
        let values: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(super::percentile(&values, 50), 50.0);
        assert_eq!(super::percentile(&values, 99), 99.0);
        assert_eq!(super::percentile(&[4.0], 99), 4.0);
    }
}
