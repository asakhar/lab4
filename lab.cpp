#include <sched.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#include <cerrno>
#include <cstddef>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

#include "errors.hpp"
#include "shared_memory.hpp"

constexpr char const *processorPath = "./processor";

#define SV(str) str, strlen(str)

void usage(int argc, char *argv[]) {
  std::cerr << "Usage:\n\t" << argv[0]
            << " file_to_process number_of_processes character_to_count\n";
  exit(1);
}

struct process {
  pid_t pid = -1;
  int pipeFwd = -1;
  int pipeBck = -1;
  process() = default;
  explicit process(char const *program) {
    int pipefd1[2];
    if (pipe(pipefd1) == -1) {
      printError("Cannot create pipe", errno);
      exit(1);
    }
    if (fcntl(pipefd1[0], F_SETFL,
              (fcntl(pipefd1[0], F_GETFL) & ~O_NONBLOCK)) == -1) {
      printError("Failed to unset O_NONBLOCK to pipe", errno);
      exit(1);
    }
    pipeFwd = pipefd1[1];
    int pipefd2[2];
    if (pipe(pipefd2) == -1) {
      printError("Cannot create pipe", errno);
      exit(1);
    }
    if (fcntl(pipefd2[0], F_SETFL,
              (fcntl(pipefd2[0], F_GETFL) & ~O_NONBLOCK)) == -1) {
      printError("Failed to unset O_NONBLOCK to pipe", errno);
      exit(1);
    }
    pipeBck = pipefd2[0];
    pid = fork();
    if (pid != 0) {
      close(pipefd1[0]);
      close(pipefd2[1]);
    } else {
      close(pipefd1[1]);
      close(pipefd2[0]);
      auto pipefdstr =
          std::to_string(pipefd1[0]) + "|" + std::to_string(pipefd2[1]);
      char const *const argv[3]{program, pipefdstr.c_str(), nullptr};
      if (execve(program, const_cast<char *const *>(argv), nullptr) == -1) {
        printError("Execve call resulted in error", errno);
        exit(1);
      }
    }
  }
  process(process &&move)
      : pipeFwd{move.pipeFwd}, pipeBck{move.pipeBck}, pid{move.pid} {
    move.pipeFwd = -1;
    move.pipeBck = -1;
    move.pid = -1;
  }
  process(process const &) = delete;
  process &operator=(process &&move) {
    endProc();
    pipeFwd = move.pipeFwd;
    pipeBck = move.pipeBck;
    pid = move.pid;
    move.pipeFwd = -1;
    move.pipeBck = -1;
    move.pid = -1;
    return *this;
  }
  process &operator=(process const &) = delete;
  void endProc() {
    if (pipeFwd != -1) close(pipeFwd);
    if (pipeBck != -1) close(pipeBck);
    int _;
    if (pid != -1) waitpid(pid, &_, 0);
  }
  ~process() { endProc(); }
};

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
  if (processorsQuantity > (fileSize >> 1)) {
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
      writeObject(processes[i].pipeFwd, strlen(argv[1]) + 1);
      writeObject(processes[i].pipeFwd, SV(argv[1]) + 1);
      writeObject(processes[i].pipeFwd, blockSize);
      writeObject(processes[i].pipeFwd, character);
      writeObject(processes[i].pipeFwd, i * blockSize);
    }
    processes[i] = process(processorPath);
    writeObject(processes[i].pipeFwd, strlen(argv[1]) + 1);
    writeObject(processes[i].pipeFwd, SV(argv[1]) + 1);
    writeObject(processes[i].pipeFwd, lastBlockSize);
    writeObject(processes[i].pipeFwd, character);
    writeObject(processes[i].pipeFwd, i * blockSize);
  }
  size_t total = 0;
  for (auto &proc : processes) {
    total += readObject<size_t>(proc.pipeBck);
  }
  std::cout << "Result for given file is: " << total << "\n";
  return 0;
}