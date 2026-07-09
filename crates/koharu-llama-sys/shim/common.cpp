#include <cstdlib>
#include <exception>
#include <string>

#include <nlohmann/json.hpp>

#include "json-schema-to-grammar.h"
#include "llama.h"
#include "wrapper_utils.h"

#ifdef _WIN32
#define KOHARU_LLAMA_SHIM_EXPORT __declspec(dllexport)
#else
#define KOHARU_LLAMA_SHIM_EXPORT __attribute__((visibility("default")))
#endif

extern "C" KOHARU_LLAMA_SHIM_EXPORT llama_rs_status llama_rs_json_schema_to_grammar(
    const char * schema_json,
    bool force_gbnf,
    char ** out_grammar) {
    if (schema_json == nullptr || out_grammar == nullptr) {
        return LLAMA_RS_STATUS_INVALID_ARGUMENT;
    }

    *out_grammar = nullptr;
    try {
        const auto schema = nlohmann::ordered_json::parse(schema_json);
        const std::string grammar = json_schema_to_grammar(schema, force_gbnf);
        *out_grammar = llama_rs_dup_string(grammar);
        return *out_grammar == nullptr
            ? LLAMA_RS_STATUS_ALLOCATION_FAILED
            : LLAMA_RS_STATUS_OK;
    } catch (const std::bad_alloc &) {
        return LLAMA_RS_STATUS_ALLOCATION_FAILED;
    } catch (...) {
        return LLAMA_RS_STATUS_EXCEPTION;
    }
}

struct llama_rs_mtp_speculative {};

extern "C" KOHARU_LLAMA_SHIM_EXPORT struct llama_rs_mtp_speculative * llama_rs_mtp_speculative_init(
    struct llama_context *,
    struct llama_context *,
    int32_t,
    int32_t,
    float) {
    return nullptr;
}

extern "C" KOHARU_LLAMA_SHIM_EXPORT void llama_rs_mtp_speculative_free(
    struct llama_rs_mtp_speculative *) {}

extern "C" KOHARU_LLAMA_SHIM_EXPORT llama_rs_status llama_rs_mtp_speculative_begin(
    struct llama_rs_mtp_speculative *,
    const llama_token *,
    size_t) {
    return LLAMA_RS_STATUS_INVALID_ARGUMENT;
}

extern "C" KOHARU_LLAMA_SHIM_EXPORT llama_rs_status llama_rs_mtp_speculative_process(
    struct llama_rs_mtp_speculative *,
    const struct llama_batch *) {
    return LLAMA_RS_STATUS_INVALID_ARGUMENT;
}

extern "C" KOHARU_LLAMA_SHIM_EXPORT llama_rs_status llama_rs_mtp_speculative_draft(
    struct llama_rs_mtp_speculative *,
    llama_pos,
    llama_token,
    const llama_token *,
    size_t,
    llama_token *,
    size_t,
    size_t *) {
    return LLAMA_RS_STATUS_INVALID_ARGUMENT;
}

extern "C" KOHARU_LLAMA_SHIM_EXPORT llama_rs_status llama_rs_mtp_speculative_accept(
    struct llama_rs_mtp_speculative *,
    uint16_t) {
    return LLAMA_RS_STATUS_INVALID_ARGUMENT;
}

extern "C" KOHARU_LLAMA_SHIM_EXPORT void llama_rs_string_free(char * ptr) {
    std::free(ptr);
}
