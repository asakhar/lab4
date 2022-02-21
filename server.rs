use std::error::Error;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process;
use std::sync::{
  atomic::{AtomicU64, Ordering},
  mpsc::channel,
  Arc, Mutex,
};
use std::thread;
use std::time::Duration;

macro_rules! error_and_exit {
  ($msg:expr, $err:expr) => {
    error_and_exit_internal(&$msg.to_string(), &$err.to_string())
  };
}

fn error_and_exit_internal(msg: &String, err: &String) -> ! {
  eprintln!("Error: {}: {}", msg, err);
  process::exit(1);
}

struct Task {
  id: u64,
  result: Option<Vec<u8>>,
  data: Vec<u8>,
}
static TASK_ID: AtomicU64 = AtomicU64::new(0);
impl Task {
  fn new(data: Vec<u8>) -> Task {
    let uid = TASK_ID.load(Ordering::Acquire);
    TASK_ID.store(0, Ordering::Release);
    Task {
      id: uid,
      result: None,
      data: data,
    }
  }
}

impl Clone for Task {
  fn clone(&self) -> Task {
    Task {
      id: self.id,
      result: self.result.clone(),
      data: self.data.clone(),
    }
  }
}
struct TasksContainer {
  idle_tasks: Arc<Mutex<Vec<Task>>>,
  succeeded_tasks: Arc<Mutex<Vec<Task>>>,
}
impl TasksContainer {
  fn new() -> TasksContainer {
    TasksContainer {
      idle_tasks: Arc::new(Mutex::new(Vec::new())),
      succeeded_tasks: Arc::new(Mutex::new(Vec::new())),
    }
  }
}

struct ClusterCoordinator {
  tasks: Arc<TasksContainer>,
  program: Arc<String>,
  thread_handle: Option<std::thread::JoinHandle<()>>,
}

#[allow(dead_code)]
enum HostCommands {
  Execute = 0,
  Wait = 1,
  Terminate = 2,
}

fn write_data(stream: &mut TcpStream, data: &[u8], program: &[u8]) -> bool {
  if match stream.write(&program.len().to_be_bytes()) {
    Err(why) => {
      println!("Error writing program length to host: {}", why);
      0
    }
    Ok(res) => res,
  } != 8
  {
    println!("Error occured.");
    return false;
  }
  if match stream.write(program) {
    Err(why) => {
      println!("Error writing program to host: {}", why);
      0
    }
    Ok(res) => res,
  } != program.len()
  {
    println!("Error occured.");
    return false;
  }
  if match stream.write(&data.len().to_be_bytes()) {
    Err(why) => {
      println!("Error writing data length to host: {}", why);
      0
    }
    Ok(res) => res,
  } != 8
  {
    println!("Error occured.");
    return false;
  }
  if match stream.write(&data) {
    Err(why) => {
      println!("Error writing data to host: {}", why);
      0
    }
    Ok(res) => res,
  } != data.len()
  {
    println!("Error occured.");
    return false;
  }
  return true;
}

fn read_results(stream: &mut TcpStream) -> Option<Vec<u8>> {
  let mut size_buf = [0u8; 8];
  match stream.read_exact(&mut size_buf) {
    Err(_why) => return None,
    Ok(()) => {}
  };
  let mut buf = vec![0u8; usize::from_be_bytes(size_buf)];
  match stream.read_exact(&mut buf) {
    Err(_why) => return None,
    Ok(()) => {}
  };
  Some(buf)
}

fn handle_client(mut stream: TcpStream, tasks: Arc<TasksContainer>, program: Arc<String>) {
  let mut idle_tasks = tasks.idle_tasks.lock().unwrap();
  let mut task = match idle_tasks.pop() {
    Some(val) => {
      stream.write(&[HostCommands::Execute as u8]).unwrap();
      val
    }
    None => {
      stream.write(&[HostCommands::Wait as u8]).unwrap();
      return;
    }
  };
  drop(idle_tasks);
  std::thread::spawn(move || {
    if !write_data(&mut stream, &task.data, program.as_bytes()) {
      let mut idle_tasks = tasks.idle_tasks.lock().unwrap();
      idle_tasks.push(task);
      return;
    };
    stream
      .set_read_timeout(Some(Duration::from_secs(120)))
      .unwrap();
    task.result = read_results(&mut stream);
    match task.result {
      None => {
        let mut idle_tasks = tasks.idle_tasks.lock().unwrap();
        idle_tasks.push(task);
      }
      Some(_) => {
        let mut succeeded_tasks = tasks.succeeded_tasks.lock().unwrap();
        succeeded_tasks.push(task);
      }
    };
  });
}

impl ClusterCoordinator {
  fn new(program: String, port: u16) -> ClusterCoordinator {
    let mut result = ClusterCoordinator {
      tasks: Arc::new(TasksContainer::new()),
      program: Arc::new(program.clone()),
      thread_handle: None,
    };
    let tasks = Arc::clone(&result.tasks);
    let program = Arc::clone(&result.program);
    result.thread_handle = Some(std::thread::spawn(move || {
      let address = std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(0, 0, 0, 0), port);
      let listener = match TcpListener::bind(address) {
        Err(why) => error_and_exit!("Failed to bind to port.", why),
        Ok(res) => res,
      };
      for stream in listener.incoming() {
        match stream {
          Ok(stream) => {
            println!(
              "New connection: {}",
              match stream.peer_addr() {
                Err(why) => error_and_exit!("Failed to resolve peer address.", why),
                Ok(res) => res,
              }
            );
            handle_client(stream, Arc::clone(&tasks), Arc::clone(&program));
          }
          Err(e) => {
            println!("Error: {}", e);
            /* connection failed */
          }
        }
      }
      drop(listener);
    }));
    result
  }
  fn add_task(&mut self, task: Vec<u8>) -> u64 {
    let mut idle_tasks = match self.tasks.idle_tasks.lock() {
      Err(why) => error_and_exit!("Can't lock idle tasks container", why),
      Ok(res) => res,
    };
    let newtask = Task::new(task);
    idle_tasks.push(newtask);
    idle_tasks.last().unwrap().id
  }
  fn extract_computed(&mut self) -> Vec<Task> {
    let mut succeeded_tasks = self.tasks.succeeded_tasks.lock().unwrap();
    let results = (*succeeded_tasks).clone();
    *succeeded_tasks = Vec::new();
    results
  }
}

macro_rules! error_and_exit_app {
  ($msg:expr, $print_usage:expr) => {
    error_and_exit_internal_app(&$msg.to_string(), $print_usage)
  };
  ($msg:expr) => {
    error_and_exit_internal_app(&$msg.to_string(), false)
  };
}

fn error_and_exit_internal_app(msg: &String, print_usage: bool) -> ! {
  eprintln!("Error: {}", msg);
  if print_usage {
    usage();
  }
  process::exit(1);
}

fn usage() {
  let args: Vec<String> = std::env::args().collect();
  println!(
    "Usage:\n\t {} <file_to_process> <number_of_processes> <character_to_count>\n",
    args[0]
  );
  process::exit(1);
}

fn main() {
  // let processor_name = "target/debug/processor".to_string();
  let args: Vec<String> = std::env::args().collect();
  if args.len() < 4 {
    error_and_exit_app!("Invalid number of arguments.", true);
  }
  if args[3].len() != 1 {
    error_and_exit_app!("Invalid argument value for character to count.", true);
  }
  let character_to_count = args[3].to_string();

  let file_name = args[1].to_string();
  let mut processors_quantity = match args[2].parse::<u64>() {
    Err(_why) => error_and_exit_app!("Invalid argument value for number of processes.", true),
    Ok(res) => res,
  };
  let file_size = match std::fs::metadata(file_name.clone()) {
    Err(_why) => error_and_exit_app!("Failed to open file.", true),
    Ok(metadata) => metadata.len(),
  };
  if file_size < 2 {
    error_and_exit_app!("Too small file.");
  }
  if processors_quantity > (file_size >> 1) {
    println!("Warning: Quantity of processes specified ({}) exceeds half of the amount of information in file ({}).\nThe actual number of processes will be reduced...", processors_quantity, file_size>>1);
    processors_quantity = file_size >> 1;
  }
  let block_size = file_size / processors_quantity;
  let last_block_size = file_size - block_size * (processors_quantity - 1);

  todo!("Add program code for host");
  let mut coord = ClusterCoordinator::new("".to_string(), 62552);
  todo!("Create tasks for hosts");
  coord.add_task("sdgfds".as_bytes().to_vec());
  let mut extracted = Vec::new();
  while extracted.len() != processors_quantity as usize {
    extracted.append(&mut coord.extract_computed());
  }
}
