#ifndef DATA_SOURCE_LAB4_SHARED_MEMORY__HPP
#define DATA_SOURCE_LAB4_SHARED_MEMORY__HPP

#include "errors.hpp"
#include <cerrno>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <memory>

template <typename T>
void writeObject(int fd, T *obj, size_t size = 1) {
  if (write(fd, reinterpret_cast<void const *>(obj), sizeof(T) * size) == -1) {
    printError("Writing object to file descriptor failed", errno);
    _exit(1);
  }
}

template <typename T>
void writeObject(int fd, T const &obj) {
  if (write(fd, reinterpret_cast<void const *>(&obj), sizeof(T)) == -1) {
    printError("Writing object to file descriptor failed", errno);
    _exit(1);
  }
}

template <typename T>
T readObject(int fd) {
  T obj;
  if (read(fd, reinterpret_cast<void *>(&obj), sizeof(T)) == -1) {
    printError("Reading object from file descriptor failed", errno);
    _exit(1);
  }
  return obj;
}

template <typename T>
auto readObject(int fd, size_t size) {
  auto obj = std::make_unique<T[]>(size);
  if (read(fd, reinterpret_cast<void *>(obj.get()), sizeof(T)*size) == -1) {
    printError("Reading object from file descriptor failed", errno);
    _exit(1);
  }
  return obj;
}

#endif // DATA_SOURCE_LAB4_SHARED_MEMORY__HPP