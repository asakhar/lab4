#ifndef DATA_SOURCE_LAB4_ERRORS__HPP
#define DATA_SOURCE_LAB4_ERRORS__HPP
#include <iostream>
#include <cstring>
#include <windows.h>

void printError(char const *errortext, DWORD errorcode);

char const *strend(char const *str);

#endif // DATA_SOURCE_LAB4_ERRORS__HPP