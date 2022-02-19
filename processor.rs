use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::io::{self, Write};
use std::process;

macro_rules! error_and_exit {
  ($msg:expr, $print_usage:expr) => {
    error_and_exit_internal(&$msg.to_string(), $print_usage)
  };
  ($msg:expr) => {
    error_and_exit_internal(&$msg.to_string(), false)
  };
}

fn error_and_exit_internal(msg: &String, print_usage: bool) -> ! {
  eprintln!("Error: {}", msg.to_string());
  if print_usage {
    usage();
  }
  process::exit(1);
}

fn usage() {
  let args: Vec<String> = env::args().collect();
  eprintln!(
    "Usage:\n\t {} <file_to_process> <block_size> <offset> <character_to_count>\n",
    args[0]
  );
  process::exit(1);
}

fn read_n<R>(reader: R, bytes_to_read: u64) -> Vec<u8>
where
  R: Read,
{
  let mut buf = vec![];
  let mut chunk = reader.take(bytes_to_read);
  let n = match chunk.read_to_end(&mut buf) {
    Err(_why) => error_and_exit!("Failed to read from file.", true),
    Ok(res) => res,
  };
  if bytes_to_read as usize != n {
    error_and_exit!("Not enought bytes to read from file.");
  }
  buf
}

fn main() {
  let args: Vec<String> = env::args().collect();
  if args.len() < 5 {
    error_and_exit!("Invalid number of arguments.", true);
  }
  let file_name = args[1].clone();
  let block_size = match args[2].parse::<u64>() {
    Err(_why) => error_and_exit!("Invalid agrument value for blocK_size.", true),
    Ok(res) => res,
  };
  let offset = match args[3].parse::<u64>() {
    Err(_why) => error_and_exit!("Invalid agrument value for offset.", true),
    Ok(res) => res,
  };
  let character_to_count = match args[4].chars().nth(0) {
    None => error_and_exit!("Invalid agrument value for character_to_count.", true),
    Some(res) => res as u8,
  };

  let mut file = match File::open(&file_name) {
    Err(_why) => error_and_exit!("Failed to open file.", true),
    Ok(file) => file,
  };
  if file.seek(SeekFrom::Start(offset)).is_err() {
    error_and_exit!("Failed to set offset in file.");
  }
  let buf = read_n(&mut file, block_size);
  let mut result: u64 = 0;
  for symbol in buf {
    if symbol == character_to_count {
      result += 1;
    }
  }
  let result_bytes = result.to_be_bytes();
  if io::stdout().write_all(&result_bytes).is_err() {
    error_and_exit!("Failed to write bytes to stdout.");
  }
}
