#![feature(unboxed_closures)]
#![feature(fn_traits)]
use std::collections::HashMap;
use std::convert::TryInto;
use std::fs::remove_file;
use std::fs::OpenOptions;
use std::io::{Error, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::process::{Command, Stdio};

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
  fn read_sized(&mut self) -> Result<Vec<u8>, Error>;
}

trait WriteSizedExt {
  fn write_sized(&mut self, data: &Vec<u8>) -> Result<(), Error>;
}

impl ReadSizedExt for TcpStream {
  fn read_sized(&mut self) -> Result<Vec<u8>, Error> {
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

impl WriteSizedExt for TcpStream {
  fn write_sized(&mut self, data: &Vec<u8>) -> Result<(), Error> {
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

struct ClientReceiver {
  cache: HashMap<Vec<u8>, String>,
  address: SocketAddrV4,
}

impl ClientReceiver {
  fn new(address: &SocketAddrV4) -> Self {
    Self {
      cache: HashMap::new(),
      address: address.clone(),
    }
  }
  fn compile(&mut self, program: &Vec<u8>) -> Result<String, Error> {
    match self.cache.get(program) {
      Some(res) => return Ok(res.clone()),
      None => (),
    };
    static EXEC_BASE: &str = "./executable";
    let mut executable_name: String = EXEC_BASE.to_string();
    {
      let mut n = 0usize;
      while match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&executable_name)
      {
        Err(_why) => {
          executable_name = EXEC_BASE.to_string();
          executable_name.push_str(&n.to_string());
          n += 1;
          true
        }
        Ok(_res) => false,
      } {}
    }
    let mut child = match Command::new("gcc")
      .args(["-o", &executable_name, "-xc", "-"])
      .stdin(Stdio::piped())
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
        match remove_file(executable_name) {
          Err(why) => eprintln!("Error happened during removing executable: {}", why),
          Ok(()) => (),
        };
        return Err(Error::new(ErrorKind::Other, "Compilation failed"));
      }
    };
    self.cache.insert(program.clone(), executable_name.clone());
    Ok(executable_name)
  }

  fn execute(executable: &String, data: &Vec<u8>) -> Result<Vec<u8>, Error> {
    let mut child = match Command::new(executable)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
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
    return Ok(output.stdout);
  }
}

impl std::ops::FnOnce<()> for ClientReceiver {
  type Output = ();
  extern "rust-call" fn call_once(self, _: ()) -> Self::Output {}
}

impl std::ops::FnMut<()> for ClientReceiver {
  extern "rust-call" fn call_mut(&mut self, _: ()) -> Self::Output {
    loop {
      match TcpStream::connect(&self.address) {
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
          let compiled = match self.compile(&program) {
            Err(why) => {
              eprintln!("Error during compilation stage: {}", why);
              continue;
            }
            Ok(res) => res,
          };
          let result = match Self::execute(&compiled, &data) {
            Ok(res) => res,
            Err(why) => {
              eprintln!(
                "Error during execution stage: {}. Removing file from cache...",
                why
              );
              self.cache.remove(&program);
              match remove_file(compiled) {
                Err(why) => eprintln!("Error happened during removing executable: {}", why),
                Ok(()) => (),
              };
              continue;
            }
          };
          match stream.write_sized(&result) {
            Ok(()) => {}
            Err(why) => eprintln!("Error writing to host: {}", why),
          }
        }
        Err(_e) => {
          // eprintln!("Failed to connect: {}", e);
        }
      }
    }
  }
}

impl Drop for ClientReceiver {
  fn drop(&mut self) {
    for compiled in self.cache.values() {
      match remove_file(compiled) {
        Err(why) => eprintln!("Error happened during removing executable: {}", why),
        Ok(()) => (),
      };
    }
  }
}

fn main() {
  let mut recv: ClientReceiver =
    ClientReceiver::new(&SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 65535));
  recv();
  println!("Terminated.");
}
