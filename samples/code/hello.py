#!/usr/bin/env python3
"""Sample Python file for Tessera's syntax-highlighted text viewer."""

def greet(name: str) -> str:
    return f"Hello, {name}!"

if __name__ == "__main__":
    for who in ("Tessera", "world"):
        print(greet(who))
