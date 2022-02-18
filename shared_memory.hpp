#ifndef DATA_SOURCE_LAB4_SHARED_MEMORY__HPP
#define DATA_SOURCE_LAB4_SHARED_MEMORY__HPP

#include "errors.hpp"
#include <memory>
#include <windows.h>


template <typename T> void writeObject(HANDLE fd, T *obj, size_t size = 1) {
  DWORD written;
  if (!WriteFile(fd, reinterpret_cast<void const *>(obj), static_cast<DWORD>(sizeof(T) * size),
                 &written, NULL) || static_cast<size_t>(written) != sizeof(T)*size) {
    printError("Writing object to file descriptor failed", GetLastError());
    ExitProcess(1);
  }
}

template <typename T> void writeObject(HANDLE fd, T const &obj) {
  DWORD written;
  if (!WriteFile(fd, reinterpret_cast<void const *>(&obj), sizeof(T), &written,
                 NULL) || written != sizeof(T)) {
    printError("Writing object to file descriptor failed", GetLastError());
    ExitProcess(1);
  }
}

template <typename T> T readObject(HANDLE fd) {
  T obj;
  DWORD read;
  if (!ReadFile(fd, reinterpret_cast<void *>(&obj), sizeof(T), &read, NULL) || read != sizeof(T)) {
    printError("Reading object from file descriptor failed", GetLastError());
    ExitProcess(1);
  }
  return obj;
}

template <typename T> auto readObject(HANDLE fd, size_t size) {
  auto obj = std::make_unique<T[]>(size);
  DWORD read;
  if (!ReadFile(fd, reinterpret_cast<void *>(obj.get()), static_cast<DWORD>(sizeof(T) * size),
                &read, NULL) || static_cast<size_t>(read) != sizeof(T)*size) {
    printError("Reading object from file descriptor failed", GetLastError());
    ExitProcess(1);
  }
  return obj;
}

#endif // DATA_SOURCE_LAB4_SHARED_MEMORY__HPP