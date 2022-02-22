use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process;
use std::sync::{
  atomic::{AtomicBool, AtomicUsize, Ordering},
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
type Uid = usize;
type Guid = usize;

pub struct Task {
  guid: Guid,
  id: Uid,
  pub result: Option<Vec<u8>>,
  pub data: Vec<u8>,
}

impl Task {
  pub fn new(data: Vec<u8>, id: Uid) -> Task {
    static TASK_ID: AtomicUsize = AtomicUsize::new(0);
    let guid = TASK_ID.fetch_add(1, Ordering::Relaxed);
    Task {
      guid: guid,
      id: id,
      result: None,
      data: data,
    }
  }
  pub fn get_guid(&self) -> Guid {
    self.guid
  }
  pub fn get_uid(&self) -> Uid {
    self.id
  }
}

struct TasksContainer {
  idle_tasks: Arc<Mutex<Vec<Task>>>,
  succeeded_tasks: Arc<Mutex<Option<Vec<Task>>>>,
  id_max: AtomicUsize,
}
impl TasksContainer {
  fn new() -> Self {
    Self {
      idle_tasks: Arc::new(Mutex::new(Vec::new())),
      succeeded_tasks: Arc::new(Mutex::new(None)),
      id_max: AtomicUsize::new(0),
    }
  }
  fn push_idle(&self, task: Task) {
    let mut idle_tasks = match self.idle_tasks.lock() {
      Err(why) => error_and_exit!("Error locking idle tasks", why),
      Ok(res) => res,
    };
    idle_tasks.push(task);
  }
  fn take_idle(&self) -> Option<Task> {
    let mut idle_tasks = match self.idle_tasks.lock() {
      Err(why) => error_and_exit!("Error locking idle tasks", why),
      Ok(res) => res,
    };
    idle_tasks.pop()
  }
  fn push_succeeded(&self, task: Task) {
    let mut succeeded_tasks = match self.succeeded_tasks.lock() {
      Err(why) => error_and_exit!("Error locking succeeded tasks", why),
      Ok(res) => res,
    };
    match succeeded_tasks.take() {
      Some(mut res) => res.push(task),
      None => {
        let mut res = Vec::new();
        res.push(task);
        *succeeded_tasks = Some(res);
      }
    }
  }
  fn take_succeeded(&self) -> Option<Vec<Task>> {
    let mut succeeded_tasks = match self.succeeded_tasks.lock() {
      Err(why) => error_and_exit!("Error locking succeeded tasks", why),
      Ok(res) => res,
    };
    succeeded_tasks.take()
  }
  fn get_new_uid(&self) -> Uid {
    self.id_max.fetch_add(1, Ordering::SeqCst)
  }
}

pub struct ClusterCoordinator {
  tasks: Arc<TasksContainer>,
  program: Arc<String>,
  thread_handle: Option<std::thread::JoinHandle<()>>,
  is_terminated: Arc<AtomicBool>,
}

#[allow(dead_code)]
enum HostCommand {
  Wait = 0,
  Execute = 1,
  Terminate = 2,
}

fn write_data(stream: &mut TcpStream, data: &[u8], program: &[u8]) -> bool {
  match stream.write_all(&program.len().to_le_bytes()) {
    Err(why) => {
      eprintln!("Error writing program length to host: {}", why);
      return false;
    }
    Ok(()) => (),
  }
  match stream.write_all(program) {
    Err(why) => {
      eprintln!("Error writing program to host: {}", why);
      return false;
    }
    Ok(()) => (),
  }
  match stream.write_all(&data.len().to_le_bytes()) {
    Err(why) => {
      eprintln!("Error writing data length to host: {}", why);
      return false;
    }
    Ok(()) => (),
  }
  match stream.write_all(&data) {
    Err(why) => {
      eprintln!("Error writing data to host: {}", why);
      return false;
    }
    Ok(()) => (),
  }
  true
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

fn handle_client(
  mut stream: TcpStream,
  tasks: Arc<TasksContainer>,
  program: Arc<String>,
  is_terminated: Arc<AtomicBool>,
) {
  if is_terminated.load(Ordering::Relaxed) {
    stream
      .write_all(&[HostCommand::Terminate as u8])
      .unwrap_or_default();
    return;
  }
  let mut task = match tasks.take_idle() {
    Some(val) => {
      stream
        .write_all(&[HostCommand::Execute as u8])
        .unwrap_or_default();
      val
    }
    None => {
      stream
        .write_all(&[HostCommand::Wait as u8])
        .unwrap_or_default();
      return;
    }
  };
  std::thread::spawn(move || {
    if !write_data(&mut stream, &task.data, program.as_bytes()) {
      tasks.push_idle(task);
      return;
    };
    stream
      .set_read_timeout(Some(Duration::from_secs(120)))
      .unwrap_or_default();
    task.result = read_results(&mut stream);
    match task.result {
      None => tasks.push_idle(task),
      Some(_) => tasks.push_succeeded(task),
    };
  });
}

impl ClusterCoordinator {
  pub fn new(program: String, port: u16) -> ClusterCoordinator {
    let mut result = ClusterCoordinator {
      tasks: Arc::new(TasksContainer::new()),
      program: Arc::new(program.clone()),
      thread_handle: None,
      is_terminated: Arc::new(AtomicBool::new(false)),
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
            handle_client(
              stream,
              Arc::clone(&tasks),
              Arc::clone(&program),
              Arc::clone(&is_terminated),
            );
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
  pub fn add_task(&mut self, task: Vec<u8>) -> Uid {
    let uid = self.tasks.get_new_uid();
    let task = Task::new(task, uid);
    self.tasks.push_idle(task);
    uid
  }
  pub fn extract_computed(&mut self) -> Option<Vec<Task>> {
    self.tasks.take_succeeded()
  }
  pub fn terminate(&self) {
    self.is_terminated.store(true, Ordering::Relaxed);
  }
}
