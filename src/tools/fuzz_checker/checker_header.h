#include <stdlib.h>

// Override the common exit functions, EXIT will throw an exception caught by the fuzzer
#define main MAIN
#define _exit EXIT
#define exit EXIT

void EXIT(int status);
