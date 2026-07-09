#include "common.h"

#include <cstddef>

std::string string_join(const std::vector<std::string> & values, const std::string & separator) {
    std::string result;
    for (size_t i = 0; i < values.size(); ++i) {
        if (i > 0) {
            result += separator;
        }
        result += values[i];
    }
    return result;
}

std::vector<std::string> string_split(const std::string & str, const std::string & delimiter) {
    std::vector<std::string> result;
    if (delimiter.empty()) {
        result.push_back(str);
        return result;
    }

    size_t start = 0;
    while (true) {
        size_t end = str.find(delimiter, start);
        if (end == std::string::npos) {
            result.push_back(str.substr(start));
            return result;
        }
        result.push_back(str.substr(start, end - start));
        start = end + delimiter.size();
    }
}

std::string string_repeat(const std::string & str, size_t n) {
    std::string result;
    result.reserve(str.size() * n);
    for (size_t i = 0; i < n; ++i) {
        result += str;
    }
    return result;
}
