use std::convert::TryInto;
use std::env;
use std::fs;
use std::io::Read;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process;
use std::sync::{mpsc::channel, Arc, Mutex};
use std::thread;

macro_rules! error_and_exit {
  ($msg:expr, $print_usage:expr) => {
    error_and_exit_internal(&$msg.to_string(), $print_usage)
  };
  ($msg:expr) => {
    error_and_exit_internal(&$msg.to_string(), false)
  };
}

fn error_and_exit_internal(msg: &String, print_usage: bool) -> ! {
  eprintln!("Error: {}", msg);
  if print_usage {
    usage();
  }
  process::exit(1);
}

fn cvt_vec_arr<T, const N: usize>(v: &[T]) -> &[T; N] {
  match v.try_into() {
    Err(_why) => error_and_exit!("Convertion from vec to array failed"),
    Ok(res) => res,
  }
}

fn usage() {
  let args: Vec<String> = env::args().collect();
  println!(
    "Usage:\n\t {} <file_to_process> <number_of_processes> <character_to_count>\n",
    args[0]
  );
  process::exit(1);
}

fn create_process(processor_name: &String, args: &Vec<String>) -> process::Child {
  match process::Command::new(processor_name)
    .args(args)
    .stdout(process::Stdio::piped())
    .spawn()
  {
    Err(_why) => error_and_exit!("Failed to start the process."),
    Ok(res) => res,
  }
}

fn handle_client(mut stream: TcpStream, result : Arc<Mutex<u64>>) {
  let mut data = [0u8; 8];
  let mut read = 0usize;
  while match stream.read(&mut data[read..]) {
    Ok(size) => {
      read += size;
      read != data.len()
    }
    Err(_) => {
      println!(
        "An error occurred, terminating connection with {}",
        stream.peer_addr().unwrap()
      );
      stream.shutdown(Shutdown::Both).unwrap();
      false
    }
  } {}
  if read == data.len() {
    let mut result = match result.lock() {
      Err(_why) => error_and_exit!("Failed to lock."),
      Ok(res) => res
    };
    *result += u64::from_be_bytes(data);
  }
}

fn main() {
  let processor_name = "target/debug/processor".to_string();
  let args: Vec<String> = env::args().collect();
  if args.len() < 4 {
    error_and_exit!("Invalid number of arguments.", true);
  }
  if args[3].len() != 1 {
    error_and_exit!("Invalid argument value for character to count.", true);
  }
  let character_to_count = args[3].to_string();

  let file_name = args[1].to_string();
  let mut processors_quantity = match args[2].parse::<u64>() {
    Err(_why) => error_and_exit!("Invalid argument value for number of processes.", true),
    Ok(res) => res,
  };
  let file_size = match fs::metadata(file_name.clone()) {
    Err(_why) => error_and_exit!("Failed to open file.", true),
    Ok(metadata) => metadata.len(),
  };
  if file_size < 2 {
    error_and_exit!("Too small file.");
  }
  if processors_quantity > (file_size >> 1) {
    println!("Warning: Quantity of processes specified ({}) exceeds half of the amount of information in file ({}).\nThe actual number of processes will be reduced...", processors_quantity, file_size>>1);
    processors_quantity = file_size >> 1;
  }
  let block_size = file_size / processors_quantity;
  let last_block_size = file_size - block_size * (processors_quantity - 1);

  let listener = match TcpListener::bind("0.0.0.0:3333") {
    Err(_why) => error_and_exit!("Failed to bind to port."),
    Ok(res) => res,
  };

  let finished_counter = Arc::new(Mutex::new(0u64));
  let result = Arc::new(Mutex::new(0u64));
  let (tx, rx) = channel();

  for stream in listener.incoming() {
    match stream {
      Ok(stream) => {
        println!(
          "New connection: {}",
          match stream.peer_addr() {
            Err(_why) => error_and_exit!("Failed to resolve peer address."),
            Ok(res) => res,
          }
        );
        let (finished_counter, result, tx) = (
          Arc::clone(&finished_counter),
          Arc::clone(&result),
          tx.clone(),
        );
        thread::spawn(move || {
          // connection succeeded
          handle_client(stream, result);
          let mut finished_counter = match finished_counter.lock() {
            Err(_why) => error_and_exit!("Cannot lock data."),
            Ok(res) => res,
          };
          *finished_counter += 1;
          if *finished_counter == processors_quantity {
            tx.send(()).unwrap();
          }
        });
      }
      Err(e) => {
        println!("Error: {}", e);
        /* connection failed */
      }
    }
  }
  if rx.recv().is_err() {
    error_and_exit!("Sender is unavaliable.");
  };

  let mut processes = Vec::new();

  for i in 0..processors_quantity - 1 {
    processes.push(create_process(
      &processor_name,
      &vec![
        file_name.clone(),
        block_size.to_string(),
        (block_size * i).to_string(),
        character_to_count.clone(),
      ],
    ));
  }
  let last_offset = block_size * (processors_quantity - 1);
  processes.push(create_process(
    &processor_name,
    &vec![
      file_name,
      last_block_size.to_string(),
      last_offset.to_string(),
      character_to_count,
    ],
  ));
  let mut result: u64 = 0;
  for proc in processes {
    let stdout = match proc.wait_with_output() {
      Err(_why) => error_and_exit!("Failed to wait for process."),
      Ok(res) => match res.status.code() {
        Some(0) => res.stdout,
        _ => error_and_exit!("One or more of child processes finished with non zero exit code."),
      },
    };
    if stdout.len() != 8 {
      error_and_exit!("Invalid data returned from clild process.");
    }
    let buf = cvt_vec_arr(&stdout);
    let res = u64::from_be_bytes(*buf);
    result += res;
  }
  println!("Computed result is: {}", result);
}
