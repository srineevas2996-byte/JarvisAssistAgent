// =====================================================================
//  J.A.R.V.I.S. Native C++ Inference Engine (jarvis-engine.exe)
//  Architecture: Pure Stdio Win32 Anonymous Pipe Subprocess
//  - Zero network sockets, zero HTTP, zero localhost ports
//  - Completely self-contained in-house llama.cpp & libmtmd runtime
//  - Persistent pre-compiled KV cache serialization (llama_state_save/load)
//  - True neural vision token generation via mtmd (CLIP / SigLIP / Qwen2-VL)
// =====================================================================

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <bcrypt.h>
#include <iostream>
#include <string>
#include <vector>
#include <sstream>
#include <fstream>
#include <chrono>
#include <iomanip>
#include <cstdint>
#include <algorithm>
#include <thread>

#pragma comment(lib, "bcrypt.lib")

// --- Native Windows BCrypt SHA-256 for Persistent KV Cache Hashing ---
static std::string calculate_sha256(const std::string& input) {
    BCRYPT_ALG_HANDLE hAlg = NULL;
    if (BCryptOpenAlgorithmProvider(&hAlg, BCRYPT_SHA256_ALGORITHM, NULL, 0) < 0) {
        return "";
    }
    DWORD hashObjSize = 0, dataLen = 0;
    BCryptGetProperty(hAlg, BCRYPT_OBJECT_LENGTH, (PBYTE)&hashObjSize, sizeof(DWORD), &dataLen, 0);
    DWORD hashLen = 0;
    BCryptGetProperty(hAlg, BCRYPT_HASH_LENGTH, (PBYTE)&hashLen, sizeof(DWORD), &dataLen, 0);

    std::vector<BYTE> hashObj(hashObjSize);
    std::vector<BYTE> hash(hashLen);
    BCRYPT_HASH_HANDLE hHash = NULL;
    if (BCryptCreateHash(hAlg, &hHash, hashObj.data(), hashObjSize, NULL, 0, 0) >= 0) {
        BCryptHashData(hHash, (PBYTE)input.data(), (ULONG)input.size(), 0);
        BCryptFinishHash(hHash, hash.data(), hashLen, 0);
        BCryptDestroyHash(hHash);
    }
    BCryptCloseAlgorithmProvider(hAlg, 0);

    std::ostringstream ss;
    for (BYTE b : hash) {
        ss << std::hex << std::setw(2) << std::setfill('0') << (int)b;
    }
    return ss.str();
}

// --- libllama C ABI Definitions ---
typedef int32_t llama_token;
typedef int32_t llama_pos;
typedef int32_t llama_seq_id;

struct llama_batch {
    int32_t n_tokens;
    llama_token * token;
    float * embd;
    llama_pos * pos;
    int32_t * n_seq_id;
    llama_seq_id ** seq_id;
    int8_t * logits;
};

typedef void (*fn_llama_backend_init)(void);
typedef void (*fn_llama_backend_free)(void);
typedef void* (*fn_ggml_backend_load)(const char* path);
typedef void (*fn_ggml_backend_load_all_from_path)(const char* path);

typedef void* (*fn_llama_model_default_params)(void* out_params);
typedef void* (*fn_llama_model_load_from_file)(const char* path, void* params);
typedef void (*fn_llama_model_free)(void* model);
typedef const void* (*fn_llama_model_get_vocab)(const void* model);

typedef void* (*fn_llama_context_default_params)(void* out_params);
typedef void* (*fn_llama_init_from_model)(void* model, void* params);
typedef void (*fn_llama_free)(void* ctx);

typedef int32_t (*fn_llama_tokenize)(const void* vocab, const char* text, int32_t text_len, llama_token* tokens, int32_t n_tokens_max, bool add_special, bool parse_special);
typedef int32_t (*fn_llama_token_to_piece)(const void* vocab, llama_token token, char* buf, int32_t length, int32_t lstrip, bool special);
typedef llama_batch (*fn_llama_batch_init)(int32_t n_tokens, int32_t embd, int32_t n_seq_max);
typedef void (*fn_llama_batch_free)(llama_batch batch);
typedef int32_t (*fn_llama_decode)(void* ctx, llama_batch batch);

typedef void* (*fn_llama_get_memory)(const void* ctx);
typedef void (*fn_llama_memory_clear)(void* mem, bool data);
typedef bool (*fn_llama_memory_seq_rm)(void* mem, int32_t seq_id, int32_t p0, int32_t p1);
typedef bool (*fn_llama_state_save_file)(void* ctx, const char* path_state, const llama_token* tokens, size_t n_token_count);
typedef bool (*fn_llama_state_load_file)(void* ctx, const char* path_state, llama_token* tokens_out, size_t n_token_capacity, size_t* n_token_count_out);

// Sampler API
struct llama_sampler_chain_params {
    bool no_perf;
};
typedef llama_sampler_chain_params (*fn_llama_sampler_chain_default_params)(void);
typedef void* (*fn_llama_sampler_chain_init)(llama_sampler_chain_params params);
typedef void (*fn_llama_sampler_chain_add)(void* chain, void* smpl);
typedef void* (*fn_llama_sampler_init_greedy)(void);
typedef void* (*fn_llama_sampler_init_temp)(float temp);
typedef llama_token (*fn_llama_sampler_sample)(void* smpl, void* ctx, int32_t idx);
typedef void (*fn_llama_sampler_accept)(void* smpl, llama_token token);
typedef void (*fn_llama_sampler_free)(void* smpl);
typedef bool (*fn_llama_vocab_is_eog)(const void* vocab, llama_token token);

// --- libmtmd C ABI Definitions for Multi-Modal Image Decoding ---
typedef void* mtmd_context;
typedef void* mtmd_bitmap;
typedef void* mtmd_input_chunks;
typedef void* mtmd_input_chunk;

struct mtmd_context_params {
    bool use_gpu;
    uint8_t _pad0[7];
    void* _reserved0;
    bool print_timings;
    uint8_t _pad1[3];
    int32_t n_threads;
    const char* image_marker;
    const char* media_marker;
    int32_t flash_attn_type;
    bool warmup;
    uint8_t _pad2[3];
    int32_t image_min_tokens;
    int32_t image_max_tokens;
    void* cb_eval;
    void* cb_eval_user_data;
    int32_t batch_max_tokens;
    uint8_t _pad3[4];
    uint8_t reserved[16];
};
static_assert(sizeof(mtmd_context_params) == 96, "mtmd_context_params size must match 96 bytes");

struct mtmd_input_text {
    const char* text;
    size_t text_len;
    bool add_special;
    bool parse_special;
};

typedef const char* (*fn_mtmd_default_marker)(void);
typedef mtmd_context_params (*fn_mtmd_context_params_default)(void);
typedef mtmd_context* (*fn_mtmd_init_from_file)(const char* mmproj_fname, const void* text_model, mtmd_context_params ctx_params);
typedef void (*fn_mtmd_free)(mtmd_context* ctx);

typedef unsigned char* (*fn_stbi_load)(const char* filename, int* x, int* y, int* comp, int req_comp);
typedef unsigned char* (*fn_stbi_load_from_memory)(const unsigned char* buffer, int len, int* x, int* y, int* comp, int req_comp);
typedef void (*fn_stbi_image_free)(void* retval_from_stbi_load);
typedef mtmd_bitmap* (*fn_mtmd_bitmap_init)(uint32_t nx, uint32_t ny, const uint8_t* data);
typedef void (*fn_mtmd_bitmap_free)(mtmd_bitmap* bitmap);
typedef uint32_t (*fn_mtmd_bitmap_get_nx)(const mtmd_bitmap* bitmap);
typedef uint32_t (*fn_mtmd_bitmap_get_ny)(const mtmd_bitmap* bitmap);
typedef size_t (*fn_mtmd_bitmap_get_n_bytes)(const mtmd_bitmap* bitmap);

typedef mtmd_input_chunks* (*fn_mtmd_input_chunks_init)(void);
typedef void (*fn_mtmd_input_chunks_free)(mtmd_input_chunks* chunks);
typedef int32_t (*fn_mtmd_helper_get_n_tokens)(const mtmd_input_chunks* chunks);

typedef int32_t (*fn_mtmd_tokenize)(const mtmd_context* ctx,
                                     mtmd_input_chunks* output,
                                     const mtmd_input_text* text,
                                     const mtmd_bitmap* const* bitmaps,
                                     size_t n_bitmaps);

typedef int32_t (*fn_mtmd_helper_eval_chunks)(mtmd_context* ctx,
                                              void* lctx,
                                              const mtmd_input_chunks* chunks,
                                              llama_pos n_past,
                                              llama_seq_id seq_id,
                                              int32_t n_batch,
                                              bool logits_last,
                                              llama_pos* new_n_past);

typedef bool (*fn_mtmd_support_vision)(const mtmd_context* ctx);

// In-Memory Base64 Decoder
static std::vector<uint8_t> Base64Decode(const std::string& in) {
    std::vector<uint8_t> out;
    if (in.empty()) return out;
    static const int T[256] = {
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,62,-1,-1,-1,63,
        52,53,54,55,56,57,58,59,60,61,-1,-1,-1, 0,-1,-1,
        -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,10,11,12,13,14,
        15,16,17,18,19,20,21,22,23,24,25,-1,-1,-1,-1,-1,
        -1,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,
        41,42,43,44,45,46,47,48,49,50,51,-1,-1,-1,-1,-1
    };
    int val = 0, valb = -8;
    for (unsigned char c : in) {
        if (T[c] == -1) {
            if (c == '=') break;
            continue;
        }
        val = (val << 6) + T[c];
        valb += 6;
        if (valb >= 0) {
            out.push_back((uint8_t)((val >> valb) & 0xFF));
            valb -= 8;
        }
    }
    return out;
}

// --- JSON Helpers ---
static std::string escape_json(const std::string& s) {
    std::ostringstream o;
    for (char c : s) {
        switch (c) {
            case '"': o << "\\\""; break;
            case '\\': o << "\\\\"; break;
            case '\b': o << "\\b"; break;
            case '\f': o << "\\f"; break;
            case '\n': o << "\\n"; break;
            case '\r': o << "\\r"; break;
            case '\t': o << "\\t"; break;
            default:
                if ('\x00' <= c && c <= '\x1f') {
                    o << "\\u" << std::hex << std::setw(4) << std::setfill('0') << (int)c;
                } else {
                    o << c;
                }
        }
    }
    return o.str();
}

static void emit_event(const std::string& event, const std::string& data = "", const std::string& model = "", const std::string& error = "", float time_taken = -1.0f) {
    std::cout << "{\"event\":\"" << event << "\"";
    if (!data.empty()) std::cout << ",\"data\":\"" << escape_json(data) << "\"";
    if (!model.empty()) std::cout << ",\"model\":\"" << escape_json(model) << "\"";
    if (!error.empty()) std::cout << ",\"error\":\"" << escape_json(error) << "\"";
    if (time_taken >= 0.0f) std::cout << ",\"time_taken\":" << time_taken;
    std::cout << "}\n";
    std::cout.flush();
}

static std::string unescape_json(const std::string& s) {
    std::string res;
    res.reserve(s.length());
    for (size_t i = 0; i < s.length(); i++) {
        if (s[i] == '\\' && i + 1 < s.length()) {
            char next = s[i+1];
            if (next == 'n') { res += '\n'; i++; }
            else if (next == 'r') { res += '\r'; i++; }
            else if (next == 't') { res += '\t'; i++; }
            else if (next == '"') { res += '"'; i++; }
            else if (next == '\\') { res += '\\'; i++; }
            else { res += next; i++; }
        } else {
            res += s[i];
        }
    }
    return res;
}

static std::string extract_json_str(const std::string& json, const std::string& key) {
    std::string needle = "\"" + key + "\":";
    size_t pos = json.find(needle);
    if (pos == std::string::npos) {
        needle = "\"" + key + "\" :";
        pos = json.find(needle);
    }
    if (pos == std::string::npos) return "";

    size_t val_start = json.find('"', pos + needle.length());
    if (val_start == std::string::npos) return "";
    val_start++;

    size_t val_end = val_start;
    bool escape = false;
    while (val_end < json.length()) {
        char c = json[val_end];
        if (escape) {
            escape = false;
        } else if (c == '\\') {
            escape = true;
        } else if (c == '"') {
            break;
        }
        val_end++;
    }
    if (val_end >= json.length()) return "";
    return unescape_json(json.substr(val_start, val_end - val_start));
}

static float extract_json_float(const std::string& json, const std::string& key, float default_val) {
    std::string needle = "\"" + key + "\":";
    size_t pos = json.find(needle);
    if (pos == std::string::npos) return default_val;
    size_t start = json.find_first_of("0123456789.-", pos + needle.length());
    if (start == std::string::npos) return default_val;
    size_t end = json.find_first_not_of("0123456789.-", start);
    try {
        return std::stof(json.substr(start, end - start));
    } catch (...) {
        return default_val;
    }
}

struct ChatMessage {
    std::string role;
    std::string content;
};

static std::vector<ChatMessage> parse_messages(const std::string& json) {
    std::vector<ChatMessage> msgs;
    size_t m_pos = json.find("\"messages\"");
    if (m_pos == std::string::npos) return msgs;

    size_t arr_start = json.find('[', m_pos);
    if (arr_start == std::string::npos) return msgs;

    size_t cur = arr_start;
    while (true) {
        size_t obj_start = json.find('{', cur);
        if (obj_start == std::string::npos) break;

        size_t obj_end = obj_start + 1;
        int depth = 1;
        bool in_str = false;
        bool escape = false;
        while (obj_end < json.length() && depth > 0) {
            char c = json[obj_end];
            if (escape) {
                escape = false;
            } else if (c == '\\') {
                escape = true;
            } else if (c == '"') {
                in_str = !in_str;
            } else if (!in_str) {
                if (c == '{') depth++;
                else if (c == '}') depth--;
            }
            obj_end++;
        }
        if (depth != 0) break;

        std::string obj_str = json.substr(obj_start, obj_end - obj_start);
        std::string role = extract_json_str(obj_str, "role");
        std::string content = extract_json_str(obj_str, "content");

        if (!role.empty()) {
            msgs.push_back({role, content});
        }
        cur = obj_end;
    }
    return msgs;
}

static std::vector<std::string> extract_json_str_array(const std::string& json_str, const std::string& key) {
    std::vector<std::string> result;
    std::string search = "\"" + key + "\"";
    size_t pos = json_str.find(search);
    if (pos == std::string::npos) return result;
    size_t open_bracket = json_str.find('[', pos + search.size());
    if (open_bracket == std::string::npos) return result;
    size_t close_bracket = json_str.find(']', open_bracket);
    if (close_bracket == std::string::npos) return result;

    std::string sub = json_str.substr(open_bracket + 1, close_bracket - open_bracket - 1);
    size_t cur = 0;
    while (cur < sub.size()) {
        size_t q1 = sub.find('"', cur);
        if (q1 == std::string::npos) break;
        size_t q2 = sub.find('"', q1 + 1);
        if (q2 == std::string::npos) break;
        result.push_back(sub.substr(q1 + 1, q2 - q1 - 1));
        cur = q2 + 1;
    }
    return result;
}

static bool check_stop_condition(const std::string& full_response, const std::vector<std::string>& stop_tokens, std::string& matched_stop) {
    for (const auto& st : stop_tokens) {
        if (!st.empty() && full_response.size() >= st.size()) {
            if (full_response.compare(full_response.size() - st.size(), st.size(), st) == 0) {
                matched_stop = st;
                return true;
            }
        }
    }
    return false;
}

// ================= Main Engine Entry =================
int main(int argc, char** argv) {
    // Disable stdout buffering for low-latency JSON event streaming over pipe
    setvbuf(stdout, nullptr, _IONBF, 0);

    std::string model_path = "";
    std::string mmproj_path = "";
    uint32_t n_ctx = 16384;
    float default_temp = 0.1f;
    int32_t n_threads = (int32_t)std::thread::hardware_concurrency();
    if (n_threads <= 0) n_threads = 8;
    else if (n_threads > 12) n_threads = 12;

    bool test_mtmd_mode = false;
    std::string test_img_path = "cache\\current_screen.png";

    for (int i = 1; i < argc; i++) {
        std::string arg = argv[i];
        if (arg == "--model" && i + 1 < argc) model_path = argv[++i];
        else if (arg == "--mmproj" && i + 1 < argc) mmproj_path = argv[++i];
        else if (arg == "--ctx" && i + 1 < argc) n_ctx = std::stoul(argv[++i]);
        else if (arg == "--temp" && i + 1 < argc) default_temp = std::stof(argv[++i]);
        else if (arg == "--threads" && i + 1 < argc) n_threads = std::stoi(argv[++i]);
        else if (arg == "--test-mtmd") {
            test_mtmd_mode = true;
            if (i + 1 < argc && argv[i+1][0] != '-') test_img_path = argv[++i];
        }
    }

    // Locate DLLs strictly from in-house directories (Zero Ollama dependence)
    char exePathBuf[MAX_PATH];
    GetModuleFileNameA(NULL, exePathBuf, MAX_PATH);
    std::string exeDir = exePathBuf;
    size_t lastSlash = exeDir.find_last_of("\\/");
    if (lastSlash != std::string::npos) {
        exeDir = exeDir.substr(0, lastSlash);
    } else {
        exeDir = ".";
    }

    std::string runtimeLibDir = exeDir;
    std::vector<std::string> candidateDirs = {
        exeDir + "\\native_lib",
        exeDir + "\\..\\native_lib",
        exeDir + "\\..\\..\\native_lib",
        ".\\native_lib",
        exeDir
    };

    for (const auto& dir : candidateDirs) {
        DWORD attr = GetFileAttributesA(dir.c_str());
        if (attr != INVALID_FILE_ATTRIBUTES && (attr & FILE_ATTRIBUTE_DIRECTORY)) {
            std::string checkDll = dir + "\\libllama.dll";
            if (GetFileAttributesA(checkDll.c_str()) != INVALID_FILE_ATTRIBUTES) {
                runtimeLibDir = dir;
                break;
            }
        }
    }

    std::string currentPath = getenv("PATH") ? getenv("PATH") : "";
    std::string newPath = runtimeLibDir + ";" + exeDir + ";" + currentPath;
    _putenv_s("PATH", newPath.c_str());
    SetDllDirectoryA(runtimeLibDir.c_str());

    // 0. Load libmtmd.dll for Multi-Modal Image Decoding Engine
    HMODULE hMtmd = LoadLibraryExA((runtimeLibDir + "\\libmtmd.dll").c_str(), NULL, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR);
    if (!hMtmd) {
        hMtmd = LoadLibraryA((exeDir + "\\libmtmd.dll").c_str());
    }

    fn_mtmd_default_marker mtmd_default_marker_fn = nullptr;
    fn_mtmd_context_params_default mtmd_context_params_default_fn = nullptr;
    fn_mtmd_init_from_file mtmd_init_from_file_fn = nullptr;
    fn_mtmd_free mtmd_free_fn = nullptr;
    fn_stbi_load stbi_load_fn = nullptr;
    fn_stbi_load_from_memory stbi_load_from_memory_fn = nullptr;
    fn_stbi_image_free stbi_image_free_fn = nullptr;
    fn_mtmd_bitmap_init mtmd_bitmap_init_fn = nullptr;
    fn_mtmd_bitmap_free mtmd_bitmap_free_fn = nullptr;
    fn_mtmd_bitmap_get_nx mtmd_bitmap_get_nx_fn = nullptr;
    fn_mtmd_bitmap_get_ny mtmd_bitmap_get_ny_fn = nullptr;
    fn_mtmd_bitmap_get_n_bytes mtmd_bitmap_get_n_bytes_fn = nullptr;
    fn_mtmd_input_chunks_init mtmd_input_chunks_init_fn = nullptr;
    fn_mtmd_input_chunks_free mtmd_input_chunks_free_fn = nullptr;
    fn_mtmd_helper_get_n_tokens mtmd_helper_get_n_tokens_fn = nullptr;
    fn_mtmd_tokenize mtmd_tokenize_fn = nullptr;
    fn_mtmd_helper_eval_chunks mtmd_helper_eval_chunks_fn = nullptr;
    fn_mtmd_support_vision mtmd_support_vision_fn = nullptr;

    if (hMtmd) {
        mtmd_default_marker_fn = (fn_mtmd_default_marker)GetProcAddress(hMtmd, "mtmd_default_marker");
        mtmd_context_params_default_fn = (fn_mtmd_context_params_default)GetProcAddress(hMtmd, "mtmd_context_params_default");
        mtmd_init_from_file_fn = (fn_mtmd_init_from_file)GetProcAddress(hMtmd, "mtmd_init_from_file");
        mtmd_free_fn = (fn_mtmd_free)GetProcAddress(hMtmd, "mtmd_free");
        stbi_load_fn = (fn_stbi_load)GetProcAddress(hMtmd, "stbi_load");
        stbi_load_from_memory_fn = (fn_stbi_load_from_memory)GetProcAddress(hMtmd, "stbi_load_from_memory");
        stbi_image_free_fn = (fn_stbi_image_free)GetProcAddress(hMtmd, "stbi_image_free");
        mtmd_bitmap_init_fn = (fn_mtmd_bitmap_init)GetProcAddress(hMtmd, "mtmd_bitmap_init");
        mtmd_bitmap_free_fn = (fn_mtmd_bitmap_free)GetProcAddress(hMtmd, "mtmd_bitmap_free");
        mtmd_bitmap_get_nx_fn = (fn_mtmd_bitmap_get_nx)GetProcAddress(hMtmd, "mtmd_bitmap_get_nx");
        mtmd_bitmap_get_ny_fn = (fn_mtmd_bitmap_get_ny)GetProcAddress(hMtmd, "mtmd_bitmap_get_ny");
        mtmd_bitmap_get_n_bytes_fn = (fn_mtmd_bitmap_get_n_bytes)GetProcAddress(hMtmd, "mtmd_bitmap_get_n_bytes");
        mtmd_input_chunks_init_fn = (fn_mtmd_input_chunks_init)GetProcAddress(hMtmd, "mtmd_input_chunks_init");
        mtmd_input_chunks_free_fn = (fn_mtmd_input_chunks_free)GetProcAddress(hMtmd, "mtmd_input_chunks_free");
        mtmd_helper_get_n_tokens_fn = (fn_mtmd_helper_get_n_tokens)GetProcAddress(hMtmd, "mtmd_helper_get_n_tokens");
        mtmd_tokenize_fn = (fn_mtmd_tokenize)GetProcAddress(hMtmd, "mtmd_tokenize");
        mtmd_helper_eval_chunks_fn = (fn_mtmd_helper_eval_chunks)GetProcAddress(hMtmd, "mtmd_helper_eval_chunks");
        mtmd_support_vision_fn = (fn_mtmd_support_vision)GetProcAddress(hMtmd, "mtmd_support_vision");
    }

    if (test_mtmd_mode) {
        if (!hMtmd || !stbi_load_fn || !mtmd_bitmap_init_fn || !mtmd_bitmap_free_fn) {
            std::cerr << "[TEST_MTMD_FAIL] Failed to load libmtmd.dll or resolve symbols from " << runtimeLibDir << std::endl;
            return 1;
        }
        int w = 0, h = 0, comp = 0;
        unsigned char* data = stbi_load_fn(test_img_path.c_str(), &w, &h, &comp, 3);
        if (!data) {
            std::cerr << "[TEST_MTMD_FAIL] stbi_load failed to decode image: " << test_img_path << std::endl;
            return 1;
        }
        mtmd_bitmap* bmp = mtmd_bitmap_init_fn((uint32_t)w, (uint32_t)h, data);
        if (!bmp) {
            stbi_image_free_fn(data);
            std::cerr << "[TEST_MTMD_FAIL] mtmd_bitmap_init failed." << std::endl;
            return 1;
        }
        uint32_t nx = mtmd_bitmap_get_nx_fn ? mtmd_bitmap_get_nx_fn(bmp) : (uint32_t)w;
        uint32_t ny = mtmd_bitmap_get_ny_fn ? mtmd_bitmap_get_ny_fn(bmp) : (uint32_t)h;
        size_t bytes = mtmd_bitmap_get_n_bytes_fn ? mtmd_bitmap_get_n_bytes_fn(bmp) : 0;
        std::cout << "[TEST_MTMD_OK] libmtmd.dll functional: decoded " << test_img_path 
                  << " (" << nx << "x" << ny << ", " << bytes << " bytes)" << std::endl;
        mtmd_bitmap_free_fn(bmp);
        stbi_image_free_fn(data);
        FreeLibrary(hMtmd);
        return 0;
    }

    if (model_path.empty()) {
        emit_event("error", "", "", "Missing required --model parameter.");
        return 1;
    }

    // 1. Load backends from ggml.dll
    HMODULE hGgml = LoadLibraryA((runtimeLibDir + "\\ggml.dll").c_str());
    if (!hGgml) {
        hGgml = LoadLibraryA((exeDir + "\\ggml.dll").c_str());
    }
    if (hGgml) {
        auto load_all = (fn_ggml_backend_load_all_from_path)GetProcAddress(hGgml, "ggml_backend_load_all_from_path");
        if (load_all) load_all(runtimeLibDir.c_str());
    }

    // 2. Load libllama.dll
    HMODULE hLlama = LoadLibraryA((runtimeLibDir + "\\libllama.dll").c_str());
    if (!hLlama) {
        hLlama = LoadLibraryA((exeDir + "\\libllama.dll").c_str());
    }
    if (!hLlama) {
        emit_event("error", "", "", "Failed to load libllama.dll from " + runtimeLibDir);
        return 1;
    }

    auto backend_init = (fn_llama_backend_init)GetProcAddress(hLlama, "llama_backend_init");
    auto backend_free = (fn_llama_backend_free)GetProcAddress(hLlama, "llama_backend_free");
    auto model_default_params = (fn_llama_model_default_params)GetProcAddress(hLlama, "llama_model_default_params");
    auto model_load = (fn_llama_model_load_from_file)GetProcAddress(hLlama, "llama_model_load_from_file");
    auto model_free = (fn_llama_model_free)GetProcAddress(hLlama, "llama_model_free");
    auto model_get_vocab = (fn_llama_model_get_vocab)GetProcAddress(hLlama, "llama_model_get_vocab");
    auto ctx_default_params = (fn_llama_context_default_params)GetProcAddress(hLlama, "llama_context_default_params");
    auto init_from_model = (fn_llama_init_from_model)GetProcAddress(hLlama, "llama_init_from_model");
    auto llama_free_ctx = (fn_llama_free)GetProcAddress(hLlama, "llama_free");
    auto tokenize_fn = (fn_llama_tokenize)GetProcAddress(hLlama, "llama_tokenize");
    auto token_to_piece_fn = (fn_llama_token_to_piece)GetProcAddress(hLlama, "llama_token_to_piece");
    auto batch_init_fn = (fn_llama_batch_init)GetProcAddress(hLlama, "llama_batch_init");
    auto batch_free_fn = (fn_llama_batch_free)GetProcAddress(hLlama, "llama_batch_free");
    auto decode_fn = (fn_llama_decode)GetProcAddress(hLlama, "llama_decode");

    auto get_memory_fn = (fn_llama_get_memory)GetProcAddress(hLlama, "llama_get_memory");
    auto mem_clear_fn = (fn_llama_memory_clear)GetProcAddress(hLlama, "llama_memory_clear");
    auto seq_rm_fn = (fn_llama_memory_seq_rm)GetProcAddress(hLlama, "llama_memory_seq_rm");
    auto state_save_fn = (fn_llama_state_save_file)GetProcAddress(hLlama, "llama_state_save_file");
    auto state_load_fn = (fn_llama_state_load_file)GetProcAddress(hLlama, "llama_state_load_file");

    auto sampler_chain_init = (fn_llama_sampler_chain_init)GetProcAddress(hLlama, "llama_sampler_chain_init");
    auto sampler_chain_default_params = (fn_llama_sampler_chain_default_params)GetProcAddress(hLlama, "llama_sampler_chain_default_params");
    auto sampler_chain_add = (fn_llama_sampler_chain_add)GetProcAddress(hLlama, "llama_sampler_chain_add");
    auto sampler_init_greedy = (fn_llama_sampler_init_greedy)GetProcAddress(hLlama, "llama_sampler_init_greedy");
    auto sampler_init_temp = (fn_llama_sampler_init_temp)GetProcAddress(hLlama, "llama_sampler_init_temp");
    auto sampler_sample = (fn_llama_sampler_sample)GetProcAddress(hLlama, "llama_sampler_sample");
    auto sampler_accept = (fn_llama_sampler_accept)GetProcAddress(hLlama, "llama_sampler_accept");
    auto sampler_free = (fn_llama_sampler_free)GetProcAddress(hLlama, "llama_sampler_free");
    auto is_eog_fn = (fn_llama_vocab_is_eog)GetProcAddress(hLlama, "llama_vocab_is_eog");

    if (!backend_init || !model_load || !init_from_model || !decode_fn) {
        emit_event("error", "", "", "Essential libllama function pointers could not be resolved.");
        return 1;
    }

    backend_init();

    // Load Model directly from GGUF binary
    char mparams[256] = {0};
    model_default_params(mparams);

    void* model = model_load(model_path.c_str(), mparams);
    if (!model) {
        emit_event("error", "", "", "Failed to load model weights from: " + model_path);
        return 1;
    }
    const void* vocab = model_get_vocab(model);

    // Create Context
    char cparams[256] = {0};
    ctx_default_params(cparams);
    *(uint32_t*)(cparams + 0) = n_ctx;
    *(uint32_t*)(cparams + 4) = 2048; // n_batch
    *(uint32_t*)(cparams + 8) = 512;  // n_ubatch
    *(int32_t*)(cparams + 16) = n_threads; // n_threads
    *(int32_t*)(cparams + 20) = n_threads; // n_threads_batch

    void* ctx = init_from_model(model, cparams);
    if (!ctx) {
        emit_event("error", "", "", "Failed to allocate llama context with " + std::to_string(n_ctx) + " tokens.");
        model_free(model);
        backend_free();
        return 1;
    }

    // Allocate Reusable Batch (up to 2048 tokens at a time)
    llama_batch batch = batch_init_fn(2048, 0, 1);

    // Retrieve unified memory handle from llama context
    void* mem = get_memory_fn ? get_memory_fn(ctx) : nullptr;

    // Initialize Multimodal Engine (mtmd) if mmproj is provided or available
    mtmd_context* mtmd_ctx = nullptr;
    std::string effective_mmproj = mmproj_path;
    if (effective_mmproj.empty()) {
        std::string baseName = model_path;
        size_t lastSlashPos = baseName.find_last_of("\\/");
        std::string dir = (lastSlashPos != std::string::npos) ? baseName.substr(0, lastSlashPos + 1) : "";
        std::string file = (lastSlashPos != std::string::npos) ? baseName.substr(lastSlashPos + 1) : baseName;
        std::string companion = dir + "mmproj-" + file;
        DWORD attr = GetFileAttributesA(companion.c_str());
        if (attr != INVALID_FILE_ATTRIBUTES && !(attr & FILE_ATTRIBUTE_DIRECTORY)) {
            effective_mmproj = companion;
        } else {
            effective_mmproj = model_path;
        }
    }

    if (mtmd_init_from_file_fn && !effective_mmproj.empty()) {
        mtmd_context_params mctx_params = {};
        if (mtmd_context_params_default_fn) {
            mctx_params = mtmd_context_params_default_fn();
        } else {
            mctx_params.use_gpu = true;
            mctx_params.media_marker = "<__media__>";
            mctx_params.flash_attn_type = -1;
            mctx_params.warmup = true;
            mctx_params.image_min_tokens = -1;
            mctx_params.image_max_tokens = -1;
        }
        mctx_params.use_gpu = true;
        mctx_params.n_threads = n_threads;
        mctx_params.batch_max_tokens = 2048;
        mctx_params.image_marker = nullptr;
        mtmd_ctx = mtmd_init_from_file_fn(effective_mmproj.c_str(), model, mctx_params);
        if (mtmd_ctx) {
            std::cerr << "  [Engine MTMD] Multimodal neural vision initialized from: " << effective_mmproj 
                      << " (" << n_threads << " compute threads, batch 2048)" << std::endl;
        }
    }

    // Create cache directory if needed
    CreateDirectoryA("cache", nullptr);

    // State tracking for Persistent KV Cache & Active Resident Tokens
    std::string cached_system_hash = "";
    std::string cached_state_file = "";
    size_t n_cached_prefix = 0;
    std::vector<llama_token> g_static_kv_tokens;
    std::vector<llama_token> g_active_kv_tokens;

    auto ensure_static_kv_cache = [&](const std::string& sys_prompt) -> bool {
        if (sys_prompt.empty()) return false;
        std::string prompt_hash = calculate_sha256(sys_prompt);
        if (cached_system_hash == prompt_hash && n_cached_prefix > 0 && !g_static_kv_tokens.empty()) {
            return true;
        }

        std::string state_path = "cache\\kv_" + prompt_hash.substr(0, 16) + ".state";
        // Check if pre-compiled static cache exists on disk
        std::ifstream test_f(state_path, std::ios::binary);
        if (state_load_fn && test_f.good()) {
            test_f.close();
            std::vector<llama_token> restored_tokens(16384);
            size_t count = 0;
            bool ok = state_load_fn(ctx, state_path.c_str(), restored_tokens.data(), restored_tokens.size(), &count);
            if (ok && count > 0 && (uint32_t)count < n_ctx) {
                n_cached_prefix = count;
                cached_system_hash = prompt_hash;
                cached_state_file = state_path;
                g_static_kv_tokens.assign(restored_tokens.begin(), restored_tokens.begin() + count);
                g_active_kv_tokens.assign(restored_tokens.begin(), restored_tokens.begin() + count);
                return true;
            }
        }

        // Clear existing KV cache and bake static system prompt to KV cache in VRAM
        if (mem && mem_clear_fn) {
            mem_clear_fn(mem, true);
        } else if (mem && seq_rm_fn) {
            seq_rm_fn(mem, -1, 0, -1);
        }
        g_static_kv_tokens.clear();
        g_active_kv_tokens.clear();

        std::string formatted_system = "<|im_start|>system\n" + sys_prompt + "<|im_end|>\n";
        std::vector<llama_token> sys_toks(16384);
        int32_t n_sys = tokenize_fn(vocab, formatted_system.c_str(), (int32_t)formatted_system.length(), sys_toks.data(), (int32_t)sys_toks.size(), true, true);
        if (n_sys > 0) {
            if ((uint32_t)n_sys >= n_ctx) {
                emit_event("error", "", "", "System prompt tokens (" + std::to_string(n_sys) + ") exceed context size (" + std::to_string(n_ctx) + ")");
                return false;
            }
            sys_toks.resize(n_sys);

            // Decode static system prompt
            size_t pos = 0;
            while (pos < sys_toks.size()) {
                int32_t chunk_size = (int32_t)std::min((size_t)512, sys_toks.size() - pos);
                batch.n_tokens = chunk_size;
                for (int i = 0; i < chunk_size; i++) {
                    batch.token[i] = sys_toks[pos + i];
                    batch.pos[i] = (int32_t)(pos + i);
                    batch.n_seq_id[i] = 1;
                    batch.seq_id[i][0] = 0;
                    batch.logits[i] = false;
                }
                if (decode_fn(ctx, batch) != 0) {
                    emit_event("error", "", "", "Failed to decode static system prompt.");
                    return false;
                }
                pos += chunk_size;
            }

            n_cached_prefix = sys_toks.size();
            cached_system_hash = prompt_hash;
            cached_state_file = state_path;
            g_static_kv_tokens = sys_toks;
            g_active_kv_tokens = sys_toks;

            // Serialize compiled state to disk for instant warm restart
            if (state_save_fn) {
                state_save_fn(ctx, state_path.c_str(), sys_toks.data(), sys_toks.size());
            }
            return true;
        }
        return false;
    };

    // Ready event
    emit_event("ready", model_path, "", "", 0.0f);

    // Stdio Anonymous Pipe REPL Command Loop
    std::string line;
    while (std::getline(std::cin, line)) {
        if (line.empty()) continue;

        std::string cmd = extract_json_str(line, "cmd");

        if (cmd == "prewarm_kv") {
            std::string sys_prompt = extract_json_str(line, "system_prompt");
            if (ensure_static_kv_cache(sys_prompt)) {
                emit_event("prewarmed", "Static KV cache locked in VRAM (" + std::to_string(n_cached_prefix) + " tokens)");
            } else {
                emit_event("error", "", "", "Failed to prewarm static KV cache.");
            }
        }
        else if (cmd == "test_mtmd") {
            std::string img_path = extract_json_str(line, "image_path");
            if (img_path.empty()) img_path = "cache\\current_screen.png";

            if (!hMtmd || !stbi_load_fn || !mtmd_bitmap_init_fn || !mtmd_bitmap_free_fn) {
                emit_event("error", "", "", "libmtmd.dll is not loaded or missing symbols.");
                continue;
            }

            int w = 0, h = 0, comp = 0;
            unsigned char* data = stbi_load_fn(img_path.c_str(), &w, &h, &comp, 3);
            if (!data) {
                emit_event("error", "", "", "stbi_load failed to decode image: " + img_path);
                continue;
            }

            mtmd_bitmap* bmp = mtmd_bitmap_init_fn((uint32_t)w, (uint32_t)h, data);
            if (!bmp) {
                stbi_image_free_fn(data);
                emit_event("error", "", "", "mtmd_bitmap_init failed for: " + img_path);
                continue;
            }

            uint32_t nx = mtmd_bitmap_get_nx_fn ? mtmd_bitmap_get_nx_fn(bmp) : (uint32_t)w;
            uint32_t ny = mtmd_bitmap_get_ny_fn ? mtmd_bitmap_get_ny_fn(bmp) : (uint32_t)h;
            size_t bytes = mtmd_bitmap_get_n_bytes_fn ? mtmd_bitmap_get_n_bytes_fn(bmp) : 0;

            std::ostringstream ss;
            ss << "libmtmd decoded " << img_path << " (" << nx << "x" << ny << ", " << bytes << " bytes)";
            emit_event("mtmd_ok", ss.str());

            mtmd_bitmap_free_fn(bmp);
            stbi_image_free_fn(data);
        }
        else if (cmd == "chat") {
            auto start_time = std::chrono::high_resolution_clock::now();
            auto msgs = parse_messages(line);
            float temp = extract_json_float(line, "temperature", default_temp);
            std::string image_base64 = extract_json_str(line, "image_base64");
            std::string image_path = extract_json_str(line, "image_path");
            std::string vision_encoder = extract_json_str(line, "vision_encoder");
            if (vision_encoder.empty()) vision_encoder = "Neural ViT";
            std::vector<std::string> stop_tokens = extract_json_str_array(line, "stop_tokens");
            if (stop_tokens.empty()) {
                stop_tokens = {"<|im_end|>", "<|endoftext|>", "\nObservation:", "\nUser:", "\nuser:"};
            } else {
                stop_tokens.push_back("\nObservation:");
                stop_tokens.push_back("\nUser:");
                stop_tokens.push_back("\nuser:");
            }

            // ========================================================
            // Path A: True Neural Multi-Modal (mtmd) Decoding Pipeline
            // ========================================================
            if ((!image_base64.empty() || !image_path.empty()) && mtmd_ctx && mtmd_tokenize_fn && mtmd_helper_eval_chunks_fn) {
                mtmd_bitmap* bmp = nullptr;
                if (!image_base64.empty() && stbi_load_from_memory_fn && mtmd_bitmap_init_fn) {
                    std::vector<uint8_t> ram_bytes = Base64Decode(image_base64);
                    if (!ram_bytes.empty()) {
                        int img_w = 0, img_h = 0, img_comp = 0;
                        unsigned char* img_data = stbi_load_from_memory_fn(ram_bytes.data(), (int)ram_bytes.size(), &img_w, &img_h, &img_comp, 3);
                        if (img_data) {
                            bmp = mtmd_bitmap_init_fn((uint32_t)img_w, (uint32_t)img_h, img_data);
                            if (stbi_image_free_fn) stbi_image_free_fn(img_data);
                        }
                    }
                } else if (!image_path.empty() && stbi_load_fn && mtmd_bitmap_init_fn) {
                    int img_w = 0, img_h = 0, img_comp = 0;
                    unsigned char* img_data = stbi_load_fn(image_path.c_str(), &img_w, &img_h, &img_comp, 3);
                    if (img_data) {
                        bmp = mtmd_bitmap_init_fn((uint32_t)img_w, (uint32_t)img_h, img_data);
                        if (stbi_image_free_fn) stbi_image_free_fn(img_data);
                    }
                }

                if (bmp) {
                    const char* marker = mtmd_default_marker_fn ? mtmd_default_marker_fn() : "<__media__>";
                    std::string prompt_text = "";
                    bool marker_inserted = false;
                    for (const auto& m : msgs) {
                        if (m.role == "system") {
                            prompt_text += "<|im_start|>system\n" + m.content + "<|im_end|>\n";
                        } else if (m.role == "user") {
                            prompt_text += "<|im_start|>user\n";
                            if (!marker_inserted) {
                                if (m.content.find(marker) == std::string::npos && m.content.find("<image>") == std::string::npos) {
                                    prompt_text += std::string(marker) + "\n";
                                }
                                marker_inserted = true;
                            }
                            prompt_text += m.content + "<|im_end|>\n";
                        } else if (m.role == "assistant") {
                            prompt_text += "<|im_start|>assistant\n" + m.content + "<|im_end|>\n";
                        }
                    }
                    if (!marker_inserted) {
                        prompt_text = "<|im_start|>user\n" + std::string(marker) + "\n<|im_end|>\n" + prompt_text;
                        marker_inserted = true;
                    }
                    prompt_text += "<|im_start|>assistant\n";

                    mtmd_input_chunks* chunks = mtmd_input_chunks_init_fn();
                    mtmd_input_text in_text;
                    in_text.text = prompt_text.data();
                    in_text.text_len = prompt_text.size();
                    in_text.add_special = true;
                    in_text.parse_special = true;
                    const mtmd_bitmap* bitmaps[1] = { bmp };

                    int32_t tok_res = mtmd_tokenize_fn(mtmd_ctx, chunks, &in_text, bitmaps, 1);
                    int32_t total_tokens = mtmd_helper_get_n_tokens_fn ? mtmd_helper_get_n_tokens_fn(chunks) : 0;

                    if (tok_res != 0 || total_tokens <= 0) {
                        if (mtmd_input_chunks_free_fn) mtmd_input_chunks_free_fn(chunks);
                        if (mtmd_bitmap_free_fn) mtmd_bitmap_free_fn(bmp);
                        emit_event("error", "", "", "mtmd_tokenize failed to produce tokens for image. Error code: " + std::to_string(tok_res));
                        continue;
                    }

                    std::ostringstream tok_msg;
                    tok_msg << vision_encoder << " Encoder: Encoded " << total_tokens << " continuous multimodal tokens into transformer sequence.";
                    emit_event("thinking", tok_msg.str());

                    // Invalidate KV cache for visual frame
                    if (mem && mem_clear_fn) mem_clear_fn(mem, true);
                    g_static_kv_tokens.clear();
                    g_active_kv_tokens.clear();

                    llama_pos new_past = 0;
                    int32_t eval_ret = mtmd_helper_eval_chunks_fn(mtmd_ctx, ctx, chunks, 0, 0, 2048, true, &new_past);

                    if (mtmd_input_chunks_free_fn) mtmd_input_chunks_free_fn(chunks);
                    if (mtmd_bitmap_free_fn) mtmd_bitmap_free_fn(bmp);

                    if (eval_ret != 0) {
                        emit_event("error", "", "", "mtmd_helper_eval_chunks failed with error code: " + std::to_string(eval_ret));
                        continue;
                    }

                    // Autoregressive sampling
                    llama_sampler_chain_params sparams = { true };
                    if (sampler_chain_default_params) {
                        sparams = sampler_chain_default_params();
                    }
                    void* chain = sampler_chain_init ? sampler_chain_init(sparams) : nullptr;
                    if (chain) {
                        if (temp > 0.05f) {
                            sampler_chain_add(chain, sampler_init_temp(temp));
                        }
                        sampler_chain_add(chain, sampler_init_greedy());
                    }

                    std::string full_response = "";
                    int32_t cur_pos = new_past;
                    for (int step = 0; step < 1024; step++) {
                        llama_token tok = sampler_sample(chain, ctx, -1);
                        sampler_accept(chain, tok);

                        if (is_eog_fn(vocab, tok)) break;

                        char piece_buf[64] = {0};
                        int piece_len = token_to_piece_fn(vocab, tok, piece_buf, sizeof(piece_buf), 0, false);
                        std::string current_token_str = (piece_len > 0) ? std::string(piece_buf, piece_len) : "";

                        full_response += current_token_str;
                        std::string matched_stop;
                        if (check_stop_condition(full_response, stop_tokens, matched_stop)) {
                            full_response = full_response.substr(0, full_response.size() - matched_stop.size());
                            break;
                        }

                        emit_event("token", current_token_str);

                        batch.n_tokens = 1;
                        batch.token[0] = tok;
                        batch.pos[0] = cur_pos++;
                        batch.n_seq_id[0] = 1;
                        batch.seq_id[0][0] = 0;
                        batch.logits[0] = 1;

                        if (decode_fn(ctx, batch) != 0) break;
                    }
                    sampler_free(chain);

                    for (const auto& st : stop_tokens) {
                        size_t pos = full_response.find(st);
                        if (pos != std::string::npos) full_response = full_response.substr(0, pos);
                    }

                    auto end_time = std::chrono::high_resolution_clock::now();
                    float elapsed = std::chrono::duration<float>(end_time - start_time).count();
                    emit_event("done", full_response, "", "", elapsed);
                    continue;
                }
            }

            // ========================================================
            // Path B: Fast Text Tokenization & KV Cache Delta Path
            // ========================================================
            std::string sys_prompt = "";
            for (const auto& m : msgs) {
                if (m.role == "system") {
                    sys_prompt = m.content;
                    break;
                }
            }

            if (!sys_prompt.empty()) {
                ensure_static_kv_cache(sys_prompt);
            }

            std::string dynamic_text = "";
            for (size_t i = 0; i < msgs.size(); ++i) {
                const auto& m = msgs[i];
                if (m.role != "system") {
                    dynamic_text += "<|im_start|>" + m.role + "\n" + m.content + "<|im_end|>\n";
                }
            }
            dynamic_text += "<|im_start|>assistant\n";

            std::vector<llama_token> dyn_tokens(16384);
            int32_t n_dyn = tokenize_fn(vocab, dynamic_text.c_str(), (int32_t)dynamic_text.length(), dyn_tokens.data(), (int32_t)dyn_tokens.size(), false, true);

            if (n_dyn <= 0) {
                emit_event("error", "", "", "Failed to tokenize conversation prompt.");
                continue;
            }
            dyn_tokens.resize(n_dyn);

            std::vector<llama_token> target_tokens;
            target_tokens.reserve(n_cached_prefix + n_dyn);
            if (n_cached_prefix > 0 && !g_static_kv_tokens.empty()) {
                target_tokens.insert(target_tokens.end(), g_static_kv_tokens.begin(), g_static_kv_tokens.end());
            } else if (n_cached_prefix > 0 && g_active_kv_tokens.size() >= n_cached_prefix) {
                target_tokens.insert(target_tokens.end(), g_active_kv_tokens.begin(), g_active_kv_tokens.begin() + n_cached_prefix);
            }
            target_tokens.insert(target_tokens.end(), dyn_tokens.begin(), dyn_tokens.end());

            size_t common_len = 0;
            while (common_len < g_active_kv_tokens.size() &&
                   common_len < target_tokens.size() &&
                   g_active_kv_tokens[common_len] == target_tokens[common_len]) {
                common_len++;
            }

            if (common_len < n_cached_prefix && !g_static_kv_tokens.empty()) {
                common_len = n_cached_prefix;
            }

            if (common_len < g_active_kv_tokens.size()) {
                if (mem && seq_rm_fn) {
                    seq_rm_fn(mem, -1, (int32_t)common_len, -1);
                }
                g_active_kv_tokens.resize(common_len);
            }

            std::ostringstream kv_info;
            kv_info << "KV Cache: locked " << common_len << " tokens in VRAM (static prefix: " 
                    << n_cached_prefix << ", delta to decode: " << (target_tokens.size() - common_len) << ")";
            emit_event("thinking", kv_info.str());

            if (target_tokens.size() + 512 >= n_ctx) {
                emit_event("error", "", "", "Total sequence length (" + std::to_string(target_tokens.size()) + ") approaches context limit (" + std::to_string(n_ctx) + ")");
                continue;
            }

            size_t n_delta = target_tokens.size() - common_len;
            size_t pos = 0;
            bool decode_ok = true;
            while (pos < n_delta) {
                int32_t chunk_size = (int32_t)std::min((size_t)2048, n_delta - pos);
                batch.n_tokens = chunk_size;
                for (int i = 0; i < chunk_size; i++) {
                    batch.token[i] = target_tokens[common_len + pos + i];
                    batch.pos[i] = (int32_t)(common_len + pos + i);
                    batch.n_seq_id[i] = 1;
                    batch.seq_id[i][0] = 0;
                    batch.logits[i] = (pos + i == n_delta - 1);
                }
                int32_t ret = decode_fn(ctx, batch);
                if (ret != 0) {
                    emit_event("error", "", "", "llama_decode failed with error code " + std::to_string(ret));
                    decode_ok = false;
                    break;
                }
                pos += chunk_size;
            }

            if (!decode_ok) continue;

            g_active_kv_tokens = target_tokens;

            llama_sampler_chain_params sparams = { true };
            if (sampler_chain_default_params) {
                sparams = sampler_chain_default_params();
            }
            void* chain = sampler_chain_init ? sampler_chain_init(sparams) : nullptr;
            if (chain) {
                if (temp > 0.05f) {
                    sampler_chain_add(chain, sampler_init_temp(temp));
                }
                sampler_chain_add(chain, sampler_init_greedy());
            }

            std::string full_response = "";
            int32_t cur_pos = (int32_t)target_tokens.size();
            std::string current_token_str = "";

            for (int step = 0; step < 1024; step++) {
                llama_token tok = sampler_sample(chain, ctx, -1);
                sampler_accept(chain, tok);

                if (is_eog_fn(vocab, tok)) break;

                char piece_buf[64] = {0};
                int piece_len = token_to_piece_fn(vocab, tok, piece_buf, sizeof(piece_buf), 0, false);
                current_token_str = (piece_len > 0) ? std::string(piece_buf, piece_len) : "";

                full_response += current_token_str;
                std::string matched_stop;
                if (check_stop_condition(full_response, stop_tokens, matched_stop)) {
                    full_response = full_response.substr(0, full_response.size() - matched_stop.size());
                    break;
                }

                emit_event("token", current_token_str);

                g_active_kv_tokens.push_back(tok);

                batch.n_tokens = 1;
                batch.token[0] = tok;
                batch.pos[0] = cur_pos++;
                batch.n_seq_id[0] = 1;
                batch.seq_id[0][0] = 0;
                batch.logits[0] = 1;

                if (decode_fn(ctx, batch) != 0) break;
            }

            sampler_free(chain);

            for (const auto& st : stop_tokens) {
                size_t pos = full_response.find(st);
                if (pos != std::string::npos) full_response = full_response.substr(0, pos);
            }

            auto end_time = std::chrono::high_resolution_clock::now();
            float elapsed = std::chrono::duration<float>(end_time - start_time).count();

            emit_event("done", full_response, "", "", elapsed);
        }
        else if (cmd == "exit") {
            emit_event("exited");
            break;
        }
        else {
            emit_event("error", "", "", "Unknown command: " + cmd);
        }
    }

    // Cleanup
    if (mtmd_ctx && mtmd_free_fn) mtmd_free_fn(mtmd_ctx);
    batch_free_fn(batch);
    llama_free_ctx(ctx);
    model_free(model);
    backend_free();
    FreeLibrary(hLlama);
    if (hGgml) FreeLibrary(hGgml);
    if (hMtmd) FreeLibrary(hMtmd);

    return 0;
}
