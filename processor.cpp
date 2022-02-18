#include <windows.h>

#include <cstdlib>
#include <fstream>
#include <ios>
#include <memory>
#include <new>
#include <sstream>
#include <winnt.h>

#include "errors.hpp"
#include "shared_memory.hpp"

int main(int argc, char const *argv[]) {
  if (argc < 2) {
    ExitProcess(1);
  }
  HANDLE inout;
  {
    DWORD const access = GENERIC_READ | GENERIC_WRITE;
    DWORD const pipeMode = OPEN_EXISTING;
    std::string const pipeName(argv[1]);
    if ((inout = CreateFile(TEXT(pipeName.c_str()), access, 0, NULL, pipeMode, 0, NULL)) == INVALID_HANDLE_VALUE) {
      printError("Cannot create pipe", GetLastError());
      ExitProcess(1);
    }
  }
  if (inout == INVALID_HANDLE_VALUE) {
    ExitProcess(1);
  }
  auto fileNameSize = readObject<size_t>(inout);
  if (fileNameSize < 1) ExitProcess(1);
  auto fileNamePtr = readObject<char>(inout, fileNameSize);
  if(!fileNamePtr) ExitProcess(1);
  auto blockSize = readObject<size_t>(inout);
  if(blockSize < 1) ExitProcess(1);
  auto searchingFor = readObject<char>(inout);
  if(!searchingFor) ExitProcess(1);
  auto offset = readObject<size_t>(inout);
  std::ifstream file{fileNamePtr.get()};
  if(!file.is_open()) ExitProcess(1);
  file.seekg(offset, std::ios_base::beg);
  size_t result = 0;
  for (size_t i = 0; i < blockSize; ++i) {
    auto character = file.get();
    if (character == searchingFor) ++result;
  }
  writeObject(inout, result);
  CloseHandle(inout);
  ExitProcess(0);
}
