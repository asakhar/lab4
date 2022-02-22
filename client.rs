use std::convert::TryInto;
use std::io::{Read, Write};
use std::net::TcpStream;

#[allow(dead_code)]
enum HostCommand {
  Wait = 0,
  Execute = 1,
  Terminate = 2,
}

impl TryFrom<u8> for HostCommand {
  type Error = ();

  fn try_from(v: u8) -> Result<Self, Self::Error> {
    match v {
      x if x == HostCommand::Execute as u8 => Ok(HostCommand::Execute),
      x if x == HostCommand::Wait as u8 => Ok(HostCommand::Wait),
      x if x == HostCommand::Terminate as u8 => Ok(HostCommand::Terminate),
      _ => Err(()),
    }
  }
}

trait ReadSizedExt {
  fn read_sized(&mut self) -> Result<Vec<u8>, std::io::Error>;
}

trait WriteSizedExt {
  fn write_sized(&mut self, data: &Vec<u8>) -> Result<(), std::io::Error>;
}

impl ReadSizedExt for std::net::TcpStream {
  fn read_sized(&mut self) -> Result<Vec<u8>, std::io::Error> {
    let mut buf_size = [0u8; 8];
    match self.read_exact(&mut buf_size) {
      Ok(()) => {}
      Err(why) => return Err(why),
    };
    let mut buf = vec![0u8; usize::from_le_bytes(buf_size)];
    match self.read_exact(&mut buf) {
      Ok(()) => {}
      Err(why) => return Err(why),
    };
    Ok(buf)
  }
}

impl WriteSizedExt for std::net::TcpStream {
  fn write_sized(&mut self, data: &Vec<u8>) -> Result<(), std::io::Error> {
    match self.write_all(&data.len().to_le_bytes()) {
      Ok(()) => {}
      Err(why) => return Err(why),
    };
    match self.write_all(&data) {
      Ok(()) => {}
      Err(why) => return Err(why),
    }
    Ok(())
  }
}

fn compile(program: &Vec<u8>) -> Result<String, std::io::Error> {
  static EXEC_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
  let mut executable_name = "./executable".to_string();
  let id = EXEC_ID.load(std::sync::atomic::Ordering::Acquire);
  executable_name.push_str(&id.to_string());
  EXEC_ID.store(id + 1, std::sync::atomic::Ordering::Release);
  let mut child = match std::process::Command::new("gcc")
    .args(["-o", &executable_name, "-xc", "-"])
    .stdin(std::process::Stdio::piped())
    .spawn()
  {
    Err(why) => return Err(why),
    Ok(res) => res,
  };
  let mut stdin = child.stdin.take().unwrap();
  match stdin.write_all(&program) {
    Ok(()) => {
      drop(stdin);
    }
    Err(why) => return Err(why),
  };
  match match child.wait() {
    Ok(res) => res,
    Err(why) => return Err(why),
  }
  .code()
  {
    Some(0) => {}
    _ => {
      return Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Compilation failed",
      ))
    }
  };
  Ok(executable_name)
}

fn execute(executable: &String, data: &Vec<u8>) -> Result<Vec<u8>, std::io::Error> {
  let mut child = match std::process::Command::new(executable)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .spawn()
  {
    Err(why) => return Err(why),
    Ok(res) => res,
  };
  let stdin = child.stdin.as_mut().unwrap();
  match stdin.write_all(&data) {
    Ok(()) => {
      drop(stdin);
    }
    Err(why) => return Err(why),
  };
  let output = match child.wait_with_output() {
    Err(why) => return Err(why),
    Ok(res) => res,
  };
  Ok(output.stdout)
}

fn main() {
  loop {
    match TcpStream::connect("localhost:65535") {
      Ok(mut stream) => {
        // println!("Successfully connected to server in port 65535");

        let mut host_command_buf = [0u8; 1];
        match stream.read_exact(&mut host_command_buf) {
          Err(why) => {
            eprintln!("Error happened during reading of host command: {}", why);
            continue;
          }
          Ok(()) => (),
        }
        let host_command = *host_command_buf.first().unwrap();
        match host_command.try_into() {
          Ok(HostCommand::Execute) => {}
          Ok(HostCommand::Wait) => {
            continue;
          }
          Ok(HostCommand::Terminate) => {
            break;
          }
          Err(()) => {
            eprintln!("Invalid HostCommand received!");
            continue;
          }
        };
        let program = match stream.read_sized() {
          Err(why) => {
            eprintln!("Error reading program from server: {}", why);
            continue;
          }
          Ok(res) => res,
        };
        let data = match stream.read_sized() {
          Err(why) => {
            eprintln!("Error reading data from server: {}", why);
            continue;
          }
          Ok(mut res) => {
            let mut dt = Vec::new();
            dt.append(&mut res.len().to_le_bytes().to_vec());
            dt.append(&mut res);
            dt
          }
        };
        let compiled = match compile(&program) {
          Err(why) => {
            eprintln!("Error during compilation stage: {}", why);
            continue;
          }
          Ok(res) => res,
        };
        let result = match execute(&compiled, &data) {
          Ok(res) => res,
          Err(why) => {
            eprintln!("Error during execution stage: {}", why);
            continue;
          }
        };
        match std::fs::remove_file(compiled) {
          Err(why) => eprintln!("Error happened during removing executable: {}", why),
          Ok(()) => (),
        };
        match stream.write_sized(&result) {
          Ok(()) => {}
          Err(why) => eprintln!("Error writing to host: {}", why),
        }
      }
      Err(e) => {
        eprintln!("Failed to connect: {}", e);
      }
    }
  }
  println!("Terminated.");
}
