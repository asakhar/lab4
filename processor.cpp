#include <unistd.h>

#include <cstdlib>
#include <fstream>
#include <ios>
#include <memory>
#include <new>
#include <sstream>

#include "errors.hpp"
#include "shared_memory.hpp"

int main(int argc, char const *argv[]) {
  if (argc < 2) {
    _exit(1);
  }
  int in, out;
  {
    std::stringstream args(argv[1]);
    args >> in;
    args.get();
    args >> out;
  }
  if (in < 3 || out < 3) {
    _exit(1);
  }
  auto fileNameSize = readObject<size_t>(in);
  if (fileNameSize < 1) _exit(1);
  auto fileNamePtr = readObject<char>(in, fileNameSize);
  if(!fileNamePtr) _exit(1);
  auto blockSize = readObject<size_t>(in);
  if(blockSize < 1) _exit(1);
  auto searchingFor = readObject<char>(in);
  if(!searchingFor) _exit(1);
  auto offset = readObject<size_t>(in);
  std::ifstream file{fileNamePtr.get()};
  if(!file.is_open()) _exit(1);
  file.seekg(offset, std::ios_base::beg);
  size_t result = 0;
  for (size_t i = 0; i < blockSize; ++i) {
    auto character = file.get();
    if (character == searchingFor) ++result;
  }
  writeObject(out, result);
  close(in);
  close(out);
  _exit(0);
}
