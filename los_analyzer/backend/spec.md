# spec.md

The following specification describes an application used to determine potential obstructions to radio signals 
between pairs of buildings in a dense urban environment. The methodology is described in detail below, but 
the high level implementation is to use rasterized Lidar data to approximate physical obstructions. This lidar data 
is supplemented with additional rasterized renderings of obstructions from other data sources.

The application is broken down into modular components in compliance with software best practices 
for ease of construction, testability, etc. This document outlines the components and their interfaces

The overall flow is:
```

```

## Overall Constraints
This project should be implemented in Python, using libraries available on PyPi as appropriate. Prefer to use imported 
libraries rather than implementing the logic wherever possible

### Project Structure
Follow standard python conventions, packaging each component into logical modules. Use a shared virtual environment
for all components. Do not use the system interpreter

### Testing
All functionality must be unit tested. Tests should be as brief as possible to expose the functionality as described here.
All tests must contain concise explanatory documentation in the form of "When X, item Y should do Z"

### Interfaces


