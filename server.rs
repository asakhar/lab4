use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process;
use std::sync::{
  atomic::{AtomicU64, Ordering},
  Arc, Mutex,
};
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
impl Task {
  fn new(data: Vec<u8>) -> Task {
    static TASK_ID: AtomicU64 = AtomicU64::new(0);
    let uid = TASK_ID.load(Ordering::Acquire);
    TASK_ID.store(uid + 1, Ordering::Release);
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
  is_terminated: Arc<Mutex<bool>>,
}

#[allow(dead_code)]
enum HostCommand {
  Wait = 0,
  Execute = 1,
  Terminate = 2,
}

fn write_data(stream: &mut TcpStream, data: &[u8], program: &[u8]) -> bool {
  if match stream.write(&program.len().to_le_bytes()) {
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
  if match stream.write(&data.len().to_le_bytes()) {
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
  let mut buf = vec![0u8; usize::from_le_bytes(size_buf)];
  match stream.read_exact(&mut buf) {
    Err(_why) => return None,
    Ok(()) => {}
  };
  Some(buf)
}

fn handle_client(mut stream: TcpStream, tasks: Arc<TasksContainer>, program: Arc<String>, is_terminated: Arc<Mutex<bool>>) {
  let is_terminated = is_terminated.lock().unwrap();
  if *is_terminated {
    stream.write_all(&[HostCommand::Terminate as u8]).unwrap_or_default();
    return;
  }
  drop(is_terminated);
  let mut idle_tasks = tasks.idle_tasks.lock().unwrap();
  let mut task = match idle_tasks.pop() {
    Some(val) => {
      stream.write_all(&[HostCommand::Execute as u8]).unwrap_or_default();
      val
    }
    None => {
      stream.write_all(&[HostCommand::Wait as u8]).unwrap_or_default();
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
      .unwrap_or_default();
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
      is_terminated: Arc::new(Mutex::new(false)),
    };
    let tasks = Arc::clone(&result.tasks);
    let program = Arc::clone(&result.program);
    let is_terminated = Arc::clone(&result.is_terminated);
    result.thread_handle = Some(std::thread::spawn(move || {
      let address = std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(0, 0, 0, 0), port);
      let listener = match TcpListener::bind(address) {
        Err(why) => error_and_exit!("Failed to bind to port.", why),
        Ok(res) => res,
      };
      for stream in listener.incoming() {
        match stream {
          Ok(stream) => {
            // println!(
            //   "New connection: {}",
            //   match stream.peer_addr() {
            //     Err(why) => error_and_exit!("Failed to resolve peer address.", why),
            //     Ok(res) => res,
            //   }
            // );
            handle_client(stream, Arc::clone(&tasks), Arc::clone(&program), Arc::clone(&is_terminated));
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
    let results = succeeded_tasks.clone();
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

fn read_n<R>(reader: &mut R, bytes_to_read: u64) -> Vec<u8>
where
  R: Read,
{
  let mut buf = vec![];
  let mut chunk = reader.take(bytes_to_read);
  let n = match chunk.read_to_end(&mut buf) {
    Err(_why) => error_and_exit_app!("Failed to read from file.", true),
    Ok(res) => res,
  };
  if bytes_to_read as usize != n {
    error_and_exit_app!("Not enought bytes to read from file.");
  }
  buf
}

fn main() {
  let args: Vec<String> = std::env::args().collect();
  if args.len() < 4 {
    error_and_exit_app!("Invalid number of arguments.", true);
  }
  if args[3].len() != 1 {
    error_and_exit_app!("Invalid argument value for character to count.", true);
  }
  let character_to_count = args[3].chars().nth(0).unwrap();

  let file_name = args[1].to_string();
  let mut processors_quantity = match args[2].parse::<u64>() {
    Err(_why) => error_and_exit_app!("Invalid argument value for number of processes.", true),
    Ok(res) => res,
  };
  let file_size = match std::fs::metadata(&file_name) {
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

  let mut file = match std::fs::File::open(file_name) {
    Err(_why) => error_and_exit_app!("Failed to open file.", true),
    Ok(file) => file,
  };

  let program = "
  #include <stdio.h>

  int main() {
    unsigned long size;
    fread(&size, sizeof(unsigned long), 1, stdin);
    char data[size];
    fread(data, 1, size, stdin);
    char* string = data+1;
    char to_find = data[0];
    unsigned long cnt = 0;
    for(unsigned long i = 0; i < size; ++i) {
      if(string[i] == to_find)
        ++cnt;
    }
    fwrite(&cnt, sizeof(unsigned long), 1, stdout);
    return 0;
  }
  ";
  let mut coord = ClusterCoordinator::new(program.to_string(), 65535);
  let mut tasks = Vec::new();
  for _ in 0..(processors_quantity - 1) {
    let mut buf = read_n(&mut file, block_size);
    buf.insert(0, character_to_count as u8);
    tasks.push(coord.add_task(buf));
    // println!("Task #{}", tasks.last().unwrap());
  }
  let mut buf = read_n(&mut file, last_block_size);
  buf.insert(0, character_to_count as u8);
  tasks.push(coord.add_task(buf));

  let mut extracted = Vec::new();
  while extracted.len() != processors_quantity as usize {
    extracted.append(&mut coord.extract_computed());
  }
  let mut cnt = 0u64;
  for task in extracted {
    let res = &task.result.unwrap()[..];
    cnt += u64::from_le_bytes(match res.try_into() {
      Ok(res) => res,
      Err(_) => error_and_exit_app!("Error converting result to u64"),
    });
  }
  println!("Result is: {}", cnt);
  *coord.is_terminated.lock().unwrap() = true;
  std::thread::sleep(Duration::from_micros(500));
}
