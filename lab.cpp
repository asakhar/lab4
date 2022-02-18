#include <cstddef>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>
#include <vector>
#include <windows.h>

#include "errors.hpp"
#include "shared_memory.hpp"

constexpr char const *processorPath = "Debug/processor.exe";

#define SV(str) str, strlen(str)

void usage(int argc, char *argv[]) {
  std::cerr << "Usage:\n\t" << argv[0]
            << " file_to_process number_of_processes character_to_count\n";
  exit(1);
}

struct process {
  PROCESS_INFORMATION pi{INVALID_HANDLE_VALUE, INVALID_HANDLE_VALUE, 0, 0};
  STARTUPINFO si{};
  HANDLE pipe = INVALID_HANDLE_VALUE;
  static const std::string pipeName;
  static size_t pipeIdx;
  process() = default;
  explicit process(char const *program) {
    DWORD const access = PIPE_ACCESS_DUPLEX;
    DWORD const pipeMode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE |
                           PIPE_WAIT; //| FILE_FLAG_FIRST_PIPE_INSTANCE ;
    auto pipeNameIdx = pipeName + std::to_string(pipeIdx++);
    if ((pipe = CreateNamedPipe(TEXT(pipeNameIdx.c_str()), access, pipeMode, 1,
                                1024, 1024, NMPWAIT_USE_DEFAULT_WAIT, NULL)) ==
        INVALID_HANDLE_VALUE) {
      printError("Cannot create pipe", GetLastError());
      exit(1);
    }
    PROCESS_INFORMATION pit{};
    auto childArgv = std::string(program) + " " + pipeNameIdx;
    auto childArgvPtr = std::make_unique<char[]>(childArgv.size() + 1);
    memcpy(childArgvPtr.get(), childArgv.c_str(), childArgv.size() + 1);
    if (!CreateProcess(program, childArgvPtr.get(), NULL, NULL, TRUE, 0, NULL,
                       NULL, &si, &pit)) {
      printError("CreateProcess call resulted in error", GetLastError());
      exit(1);
    }
    if (!ConnectNamedPipe(pipe, NULL)) {
      printError("Failed to connect to pipe", GetLastError());
      exit(1);
    }
    pi = pit;
  }
  process(process &&move) : pipe{move.pipe}, pi{move.pi}, si{move.si} {
    move.pipe = INVALID_HANDLE_VALUE;
    move.pi = {INVALID_HANDLE_VALUE, INVALID_HANDLE_VALUE, 0, 0};
    move.si = {};
  }
  process(process const &) = delete;
  process &operator=(process &&move) {
    endProc();
    pipe = move.pipe;
    pi = move.pi;
    si = move.si;
    move.pipe = INVALID_HANDLE_VALUE;
    move.pi = {INVALID_HANDLE_VALUE, INVALID_HANDLE_VALUE, 0, 0};
    move.si = {};
    return *this;
  }
  process &operator=(process const &) = delete;
  void endProc() {
    if (pipe != INVALID_HANDLE_VALUE)
      DisconnectNamedPipe(pipe);
    if (pi.hProcess != INVALID_HANDLE_VALUE)
      WaitForSingleObject(pi.hProcess, INFINITE);
  }
  ~process() { endProc(); }
};

const std::string process::pipeName = "\\\\.\\pipe\\dunderFile";
size_t process::pipeIdx = 0;

int main(int argc, char *argv[], char * /*env*/[]) {
  if (argc < 4) {
    usage(argc, argv);
  }
  size_t processorsQuantity;
  {
    char *end = nullptr;
    processorsQuantity = std::strtoul(argv[2], &end, 10);
    if (strend(argv[2]) != end || processorsQuantity < 1) {
      printError("Invalid argument value for number_of_processes", 0);
      usage(argc, argv);
    }
  }
  if (strlen(argv[3]) != 1) {
    printError("Invalid argument value for character_to_count", 0);
    usage(argc, argv);
  }
  auto character = argv[3][0];
  off_t fileSize;
  {
    struct stat st;
    if (stat(argv[1], &st) == -1) {
      printError("Invalid file provided", errno);
      usage(argc, argv);
    }
    fileSize = st.st_size;
    if (fileSize < 2) {
      printError("Invalid file contents: too little symbols in file", 0);
      usage(argc, argv);
    }
  }
  if (processorsQuantity > static_cast<size_t>(fileSize >> 1)) {
    std::cout
        << "Quantity of processes you entered (" << processorsQuantity
        << ") exceeds half of the amount of data (" << (fileSize >> 1)
        << ") to be processed. Actual number of processes will be reduced.\n";
    processorsQuantity = fileSize >> 1;
  }
  size_t blockSize = fileSize / processorsQuantity;
  size_t lastBlockSize = fileSize - blockSize * (processorsQuantity - 1);
  std::vector<process> processes(processorsQuantity);
  {
    size_t i = 0;
    for (; i < processorsQuantity - 1; ++i) {
      processes[i] = process(processorPath);
      writeObject(processes[i].pipe, strlen(argv[1]) + 1);
      writeObject(processes[i].pipe, SV(argv[1]) + 1);
      writeObject(processes[i].pipe, blockSize);
      writeObject(processes[i].pipe, character);
      writeObject(processes[i].pipe, i * blockSize);
    }
    processes[i] = process(processorPath);
    writeObject(processes[i].pipe, strlen(argv[1]) + 1);
    writeObject(processes[i].pipe, SV(argv[1]) + 1);
    writeObject(processes[i].pipe, lastBlockSize);
    writeObject(processes[i].pipe, character);
    writeObject(processes[i].pipe, i * blockSize);
  }
  size_t total = 0;
  for (auto &proc : processes) {
    total += readObject<size_t>(proc.pipe);
  }
  std::cout << "Result for given file is: " << total << "\n";
  return 0;
}