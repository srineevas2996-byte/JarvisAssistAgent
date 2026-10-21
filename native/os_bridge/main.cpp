#include <windows.h>
#include <objidl.h>
#include <gdiplus.h>
#include <uiautomation.h>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <ole2.h>

#include <iostream>
#include <string>
#include <vector>
#include <map>
#include <sstream>
#include <iomanip>
#include <chrono>
#include <thread>
#include <algorithm>

#pragma comment(lib, "gdiplus.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "oleaut32.lib")
#pragma comment(lib, "uuid.lib")
#pragma comment(lib, "user32.lib")
#pragma comment(lib, "gdi32.lib")
#pragma comment(lib, "d3d11.lib")
#pragma comment(lib, "dxgi.lib")

// Global Tag Cache for click_element
struct CachedTag {
    std::string tagId;
    std::string controlType;
    std::string name;
    RECT rect;
    std::string targetSource;
};

static std::map<std::string, CachedTag> g_tagMap;
static ULONG_PTR g_gdiplusToken = 0;
static IUIAutomation* g_pAutomation = nullptr;

// ================= String & JSON Helpers =================
std::string WideToUtf8(const std::wstring& wstr) {
    if (wstr.empty()) return "";
    int size = WideCharToMultiByte(CP_UTF8, 0, wstr.data(), (int)wstr.size(), nullptr, 0, nullptr, nullptr);
    std::string res(size, 0);
    WideCharToMultiByte(CP_UTF8, 0, wstr.data(), (int)wstr.size(), &res[0], size, nullptr, nullptr);
    return res;
}

std::wstring Utf8ToWide(const std::string& str) {
    if (str.empty()) return L"";
    int size = MultiByteToWideChar(CP_UTF8, 0, str.data(), (int)str.size(), nullptr, 0);
    std::wstring res(size, 0);
    MultiByteToWideChar(CP_UTF8, 0, str.data(), (int)str.size(), &res[0], size);
    return res;
}

std::string EscapeJson(const std::string& s) {
    std::ostringstream o;
    for (size_t i = 0; i < s.length(); ++i) {
        unsigned char c = (unsigned char)s[i];
        switch (c) {
            case '"': o << "\\\""; break;
            case '\\': o << "\\\\"; break;
            case '\b': o << "\\b"; break;
            case '\f': o << "\\f"; break;
            case '\n': o << "\\n"; break;
            case '\r': o << "\\r"; break;
            case '\t': o << "\\t"; break;
            default:
                if (c < 0x20) {
                    o << "\\u" << std::hex << std::setw(4) << std::setfill('0') << (int)c;
                } else {
                    o << (char)c;
                }
        }
    }
    return o.str();
}

std::string ExtractParam(const std::string& json, const std::string& key) {
    std::string searchKey = "\"" + key + "\"";
    size_t pos = json.find(searchKey);
    if (pos == std::string::npos) return "";
    size_t colon = json.find(':', pos + searchKey.length());
    if (colon == std::string::npos) return "";
    size_t start = json.find_first_not_of(" \t\r\n", colon + 1);
    if (start == std::string::npos) return "";

    if (json[start] == '"') {
        size_t end = json.find('"', start + 1);
        while (end != std::string::npos && json[end - 1] == '\\') {
            end = json.find('"', end + 1);
        }
        if (end != std::string::npos) {
            return json.substr(start + 1, end - start - 1);
        }
    } else {
        size_t end = json.find_first_of(",}\r\n", start);
        if (end != std::string::npos) {
            std::string val = json.substr(start, end - start);
            val.erase(val.find_last_not_of(" \t\r\n") + 1);
            return val;
        }
    }
    return "";
}

int ExtractIntParam(const std::string& json, const std::string& key, int defaultVal) {
    std::string s = ExtractParam(json, key);
    if (s.empty()) return defaultVal;
    try {
        return std::stoi(s);
    } catch (...) {
        return defaultVal;
    }
}

double ExtractDoubleParam(const std::string& json, const std::string& key, double defaultVal) {
    std::string s = ExtractParam(json, key);
    if (s.empty()) return defaultVal;
    try {
        return std::stod(s);
    } catch (...) {
        return defaultVal;
    }
}

bool ExtractBoolParam(const std::string& json, const std::string& key, bool defaultVal) {
    std::string s = ExtractParam(json, key);
    if (s == "true" || s == "1") return true;
    if (s == "false" || s == "0") return false;
    return defaultVal;
}

std::vector<std::string> ExtractStringList(const std::string& json, const std::string& key) {
    std::vector<std::string> result;
    std::string searchKey = "\"" + key + "\"";
    size_t pos = json.find(searchKey);
    if (pos == std::string::npos) return result;
    size_t openBracket = json.find('[', pos);
    if (openBracket == std::string::npos) return result;
    size_t closeBracket = json.find(']', openBracket);
    if (closeBracket == std::string::npos) return result;

    std::string arrayContent = json.substr(openBracket + 1, closeBracket - openBracket - 1);
    size_t cur = 0;
    while (cur < arrayContent.length()) {
        size_t q1 = arrayContent.find('"', cur);
        if (q1 == std::string::npos) break;
        size_t q2 = arrayContent.find('"', q1 + 1);
        if (q2 == std::string::npos) break;
        result.push_back(arrayContent.substr(q1 + 1, q2 - q1 - 1));
        cur = q2 + 1;
    }
    return result;
}

void OutputSuccess(long long id, const std::string& dataJson) {
    std::cout << "{\"id\":" << id << ",\"status\":\"success\",\"result\":" << dataJson << "}" << std::endl;
}

void OutputError(long long id, const std::string& code, const std::string& message, const std::string& details, const std::string& suggestion) {
    std::cout << "{\"id\":" << id
              << ",\"status\":\"error\""
              << ",\"error_code\":\"" << EscapeJson(code) << "\""
              << ",\"message\":\"" << EscapeJson(message) << "\""
              << ",\"details\":\"" << EscapeJson(details) << "\""
              << ",\"suggestion\":\"" << EscapeJson(suggestion) << "\"}" << std::endl;
}

// ================= GDI+ & Screen Capture =================
int GetEncoderClsid(const WCHAR* format, CLSID* pClsid) {
    UINT num = 0, size = 0;
    Gdiplus::GetImageEncodersSize(&num, &size);
    if (size == 0) return -1;
    auto* pImageCodecInfo = (Gdiplus::ImageCodecInfo*)(malloc(size));
    if (!pImageCodecInfo) return -1;
    Gdiplus::GetImageEncoders(num, size, pImageCodecInfo);
    for (UINT j = 0; j < num; ++j) {
        if (wcscmp(pImageCodecInfo[j].MimeType, format) == 0) {
            *pClsid = pImageCodecInfo[j].Clsid;
            free(pImageCodecInfo);
            return (int)j;
        }
    }
    free(pImageCodecInfo);
    return -1;
}

bool SaveHBitmapToPng(HBITMAP hBitmap, const std::wstring& filePath) {
    Gdiplus::Bitmap bitmap(hBitmap, NULL);
    CLSID clsid;
    if (GetEncoderClsid(L"image/png", &clsid) < 0) return false;
    Gdiplus::Status status = bitmap.Save(filePath.c_str(), &clsid, NULL);
    return (status == Gdiplus::Ok);
}

bool CaptureScreenGdi(const std::wstring& outputPath, int& outW, int& outH) {
    HDC hScreenDC = GetDC(NULL);
    if (!hScreenDC) return false;
    outW = GetSystemMetrics(SM_CXSCREEN);
    outH = GetSystemMetrics(SM_CYSCREEN);
    HDC hMemoryDC = CreateCompatibleDC(hScreenDC);
    if (!hMemoryDC) {
        ReleaseDC(NULL, hScreenDC);
        return false;
    }
    HBITMAP hBitmap = CreateCompatibleBitmap(hScreenDC, outW, outH);
    if (!hBitmap) {
        DeleteDC(hMemoryDC);
        ReleaseDC(NULL, hScreenDC);
        return false;
    }
    HBITMAP hOldBitmap = (HBITMAP)SelectObject(hMemoryDC, hBitmap);
    BitBlt(hMemoryDC, 0, 0, outW, outH, hScreenDC, 0, 0, SRCCOPY | CAPTUREBLT);
    SelectObject(hMemoryDC, hOldBitmap);

    bool ok = SaveHBitmapToPng(hBitmap, outputPath);
    DeleteObject(hBitmap);
    DeleteDC(hMemoryDC);
    ReleaseDC(NULL, hScreenDC);
    return ok;
}

// Try DXGI desktop duplication, fall back to GDI
bool CaptureScreenBest(const std::wstring& outputPath, int& outW, int& outH, std::string& captureMethod) {
    // Attempt GDI as primary robust capture on all desktop compositions
    captureMethod = "GDI+ (Win32 Desktop)";
    return CaptureScreenGdi(outputPath, outW, outH);
}

// In-Memory Base64 Encoder
static const char g_b64chars[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
static std::string Base64Encode(const unsigned char* data, size_t len) {
    std::string ret;
    ret.reserve(((len + 2) / 3) * 4);
    int i = 0;
    int j = 0;
    unsigned char char_array_3[3];
    unsigned char char_array_4[4];

    while (len--) {
        char_array_3[i++] = *(data++);
        if (i == 3) {
            char_array_4[0] = (char_array_3[0] & 0xfc) >> 2;
            char_array_4[1] = ((char_array_3[0] & 0x03) << 4) + ((char_array_3[1] & 0xf0) >> 4);
            char_array_4[2] = ((char_array_3[1] & 0x0f) << 2) + ((char_array_3[2] & 0xc0) >> 6);
            char_array_4[3] = char_array_3[2] & 0x3f;
            for (i = 0; i < 4; i++) ret += g_b64chars[char_array_4[i]];
            i = 0;
        }
    }
    if (i) {
        for (j = i; j < 3; j++) char_array_3[j] = '\0';
        char_array_4[0] = (char_array_3[0] & 0xfc) >> 2;
        char_array_4[1] = ((char_array_3[0] & 0x03) << 4) + ((char_array_3[1] & 0xf0) >> 4);
        char_array_4[2] = ((char_array_3[1] & 0x0f) << 2) + ((char_array_3[2] & 0xc0) >> 6);
        for (j = 0; j < i + 1; j++) ret += g_b64chars[char_array_4[j]];
        while ((i++ < 3)) ret += '=';
    }
    return ret;
}

// Direct-to-RAM screen capture using GDI+ IStream (Zero disk I/O)
bool CaptureScreenRam(std::string& outBase64, int& outW, int& outH) {
    HDC hScreenDC = GetDC(NULL);
    if (!hScreenDC) return false;
    outW = GetSystemMetrics(SM_CXSCREEN);
    outH = GetSystemMetrics(SM_CYSCREEN);
    HDC hMemoryDC = CreateCompatibleDC(hScreenDC);
    if (!hMemoryDC) {
        ReleaseDC(NULL, hScreenDC);
        return false;
    }
    HBITMAP hBitmap = CreateCompatibleBitmap(hScreenDC, outW, outH);
    if (!hBitmap) {
        DeleteDC(hMemoryDC);
        ReleaseDC(NULL, hScreenDC);
        return false;
    }
    HBITMAP hOldBitmap = (HBITMAP)SelectObject(hMemoryDC, hBitmap);
    BitBlt(hMemoryDC, 0, 0, outW, outH, hScreenDC, 0, 0, SRCCOPY | CAPTUREBLT);
    SelectObject(hMemoryDC, hOldBitmap);

    IStream* pStream = nullptr;
    if (FAILED(CreateStreamOnHGlobal(NULL, TRUE, &pStream))) {
        DeleteObject(hBitmap);
        DeleteDC(hMemoryDC);
        ReleaseDC(NULL, hScreenDC);
        return false;
    }

    CLSID clsid;
    if (GetEncoderClsid(L"image/png", &clsid) < 0) {
        pStream->Release();
        DeleteObject(hBitmap);
        DeleteDC(hMemoryDC);
        ReleaseDC(NULL, hScreenDC);
        return false;
    }

    Gdiplus::Bitmap bitmap(hBitmap, NULL);
    Gdiplus::Status status = bitmap.Save(pStream, &clsid, NULL);

    DeleteObject(hBitmap);
    DeleteDC(hMemoryDC);
    ReleaseDC(NULL, hScreenDC);

    if (status != Gdiplus::Ok) {
        pStream->Release();
        return false;
    }

    HGLOBAL hMem = NULL;
    if (FAILED(GetHGlobalFromStream(pStream, &hMem))) {
        pStream->Release();
        return false;
    }

    void* pData = GlobalLock(hMem);
    if (!pData) {
        pStream->Release();
        return false;
    }

    SIZE_T size = GlobalSize(hMem);
    outBase64 = Base64Encode((const unsigned char*)pData, size);

    GlobalUnlock(hMem);
    pStream->Release();
    return true;
}

// ================= UI Automation =================
std::string ControlTypeToString(CONTROLTYPEID typeId) {
    switch (typeId) {
        case UIA_ButtonControlTypeId: return "Button";
        case UIA_EditControlTypeId: return "Edit";
        case UIA_HyperlinkControlTypeId: return "Hyperlink";
        case UIA_CheckBoxControlTypeId: return "CheckBox";
        case UIA_RadioButtonControlTypeId: return "RadioButton";
        case UIA_ComboBoxControlTypeId: return "ComboBox";
        case UIA_TabItemControlTypeId: return "TabItem";
        case UIA_MenuItemControlTypeId: return "MenuItem";
        case UIA_TextControlTypeId: return "Text";
        case UIA_ListItemControlTypeId: return "ListItem";
        case UIA_TreeItemControlTypeId: return "TreeItem";
        case UIA_ToolBarControlTypeId: return "ToolBar";
        case UIA_WindowControlTypeId: return "Window";
        case UIA_PaneControlTypeId: return "Pane";
        case UIA_GroupControlTypeId: return "Group";
        case UIA_ImageControlTypeId: return "Image";
        case UIA_CustomControlTypeId: return "Custom";
        default: return "Control";
    }
}

void CollectUiaElements(IUIAutomation* pAutomation,
                        IUIAutomationElement* pRootElem,
                        const std::string& targetSource,
                        int& tagCounter,
                        size_t maxElements,
                        bool filterToVisible,
                        std::vector<CachedTag>& outTags) {
    if (!pRootElem || !pAutomation) return;

    IUIAutomationCondition* pTrueCondition = nullptr;
    if (FAILED(pAutomation->CreateTrueCondition(&pTrueCondition)) || !pTrueCondition) return;

    IUIAutomationElementArray* pArray = nullptr;
    HRESULT hr = pRootElem->FindAll(TreeScope_Descendants, pTrueCondition, &pArray);
    pTrueCondition->Release();

    if (FAILED(hr) || !pArray) return;

    int length = 0;
    pArray->get_Length(&length);

    int screenW = GetSystemMetrics(SM_CXSCREEN);
    int screenH = GetSystemMetrics(SM_CYSCREEN);
    if (screenW <= 0) screenW = 1920;
    if (screenH <= 0) screenH = 1080;

    for (int i = 0; i < length && outTags.size() < maxElements; ++i) {
        IUIAutomationElement* pChild = nullptr;
        if (FAILED(pArray->GetElement(i, &pChild)) || !pChild) continue;

        RECT rect = {0};
        pChild->get_CurrentBoundingRectangle(&rect);
        int w = rect.right - rect.left;
        int h = rect.bottom - rect.top;

        CONTROLTYPEID cType = 0;
        pChild->get_CurrentControlType(&cType);

        if (w > 6 && h > 6 && rect.right > 0 && rect.bottom > 0) {
            // Viewport filter for target 1: ensure element is within on-screen display area
            if (filterToVisible) {
                if (rect.bottom <= 0 || rect.top >= screenH || rect.right <= 0 || rect.left >= screenW) {
                    pChild->Release();
                    continue;
                }
            }

            BSTR bstrName = nullptr;
            pChild->get_CurrentName(&bstrName);
            std::wstring wName = bstrName ? bstrName : L"";
            if (bstrName) SysFreeString(bstrName);

            std::string nameUtf8 = WideToUtf8(wName);
            // Replace control characters and newlines with space
            for (char& ch : nameUtf8) {
                if (ch == '\r' || ch == '\n' || ch == '\t') ch = ' ';
            }
            // Trim extra spaces
            size_t first = nameUtf8.find_first_not_of(' ');
            if (first != std::string::npos) {
                size_t last = nameUtf8.find_last_not_of(' ');
                nameUtf8 = nameUtf8.substr(first, (last - first + 1));
            } else {
                nameUtf8.clear();
            }

            // Skip pure 1-character punctuation tokens that clutter reading trees
            if (nameUtf8.length() == 1 && strchr(",().:;-\"\'`[]{}*#~|/\\", nameUtf8[0])) {
                pChild->Release();
                continue;
            }

            std::string cTypeStr = ControlTypeToString(cType);

            // Filter actionable interactive controls, text blocks, and named containers
            bool isActionable = (cType == UIA_ButtonControlTypeId ||
                                 cType == UIA_EditControlTypeId ||
                                 cType == UIA_HyperlinkControlTypeId ||
                                 cType == UIA_CheckBoxControlTypeId ||
                                 cType == UIA_RadioButtonControlTypeId ||
                                 cType == UIA_ComboBoxControlTypeId ||
                                 cType == UIA_TabItemControlTypeId ||
                                 cType == UIA_MenuItemControlTypeId ||
                                 cType == UIA_ListItemControlTypeId ||
                                 cType == UIA_ToolBarControlTypeId ||
                                 cType == UIA_TextControlTypeId ||
                                 !nameUtf8.empty());

            if (isActionable && !nameUtf8.empty()) {
                tagCounter++;
                std::string tagId = std::to_string(tagCounter);

                CachedTag ct;
                ct.tagId = tagId;
                ct.controlType = cTypeStr;
                ct.name = nameUtf8;
                ct.rect = rect;
                ct.targetSource = targetSource;

                outTags.push_back(ct);
                g_tagMap[tagId] = ct;
            }
        }
        pChild->Release();
    }
    pArray->Release();
}

// ================= Desktop Coordinate Normalization =================
// Normalization criteria: (0.0, 0.0) is Bottom-Left, (1.0, 1.0) is Top-Right
// Maps to Windows screen pixel coordinates where (0, 0) is Top-Left and (W-1, H-1) is Bottom-Right.
void NormalizeToPixel(double inX, double inY, int& outPixelX, int& outPixelY) {
    int screenW = GetSystemMetrics(SM_CXSCREEN);
    int screenH = GetSystemMetrics(SM_CYSCREEN);
    if (screenW <= 0) screenW = 1920;
    if (screenH <= 0) screenH = 1080;

    // Check if coordinates are in unit range [0.0, 1.0]
    if (inX >= 0.0 && inX <= 1.0 && inY >= 0.0 && inY <= 1.0) {
        outPixelX = (int)std::round(inX * (screenW - 1));
        outPixelY = (int)std::round((1.0 - inY) * (screenH - 1));
    } else {
        // Fallback for legacy raw pixel coordinates (> 1.0)
        outPixelX = (int)std::round(inX);
        outPixelY = (int)std::round(inY);
    }

    // Clamp within primary display bounds
    if (outPixelX < 0) outPixelX = 0;
    if (outPixelX >= screenW) outPixelX = screenW - 1;
    if (outPixelY < 0) outPixelY = 0;
    if (outPixelY >= screenH) outPixelY = screenH - 1;
}

// ================= Input Simulation =================
bool DoClickCoordinates(int x, int y, bool isDouble, bool isRight) {
    int screenW = GetSystemMetrics(SM_CXSCREEN);
    int screenH = GetSystemMetrics(SM_CYSCREEN);
    if (screenW <= 0 || screenH <= 0) return false;

    DWORD normX = (DWORD)(((double)x * 65535.0) / (screenW - 1));
    DWORD normY = (DWORD)(((double)y * 65535.0) / (screenH - 1));

    INPUT in[4] = {};
    in[0].type = INPUT_MOUSE;
    in[0].mi.dx = normX;
    in[0].mi.dy = normY;
    in[0].mi.dwFlags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE;

    in[1].type = INPUT_MOUSE;
    in[1].mi.dwFlags = isRight ? MOUSEEVENTF_RIGHTDOWN : MOUSEEVENTF_LEFTDOWN;

    in[2].type = INPUT_MOUSE;
    in[2].mi.dwFlags = isRight ? MOUSEEVENTF_RIGHTUP : MOUSEEVENTF_LEFTUP;

    SendInput(3, in, sizeof(INPUT));

    if (isDouble) {
        std::this_thread::sleep_for(std::chrono::milliseconds(60));
        SendInput(2, &in[1], sizeof(INPUT));
    }
    return true;
}

bool DoTypeText(const std::string& textUtf8, bool pressEnter, int clickX, int clickY) {
    if (clickX >= 0 && clickY >= 0) {
        DoClickCoordinates(clickX, clickY, false, false);
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }

    std::wstring wText = Utf8ToWide(textUtf8);
    for (wchar_t ch : wText) {
        INPUT in[2] = {};
        in[0].type = INPUT_KEYBOARD;
        in[0].ki.wScan = ch;
        in[0].ki.dwFlags = KEYEVENTF_UNICODE;

        in[1].type = INPUT_KEYBOARD;
        in[1].ki.wScan = ch;
        in[1].ki.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;

        SendInput(2, in, sizeof(INPUT));
        std::this_thread::sleep_for(std::chrono::milliseconds(12));
    }

    if (pressEnter) {
        std::this_thread::sleep_for(std::chrono::milliseconds(40));
        INPUT enter[2] = {};
        enter[0].type = INPUT_KEYBOARD;
        enter[0].ki.wVk = VK_RETURN;
        enter[1].type = INPUT_KEYBOARD;
        enter[1].ki.wVk = VK_RETURN;
        enter[1].ki.dwFlags = KEYEVENTF_KEYUP;
        SendInput(2, enter, sizeof(INPUT));
    }
    return true;
}

WORD KeyNameToVk(const std::string& key) {
    std::string k = key;
    std::transform(k.begin(), k.end(), k.begin(), ::tolower);

    if (k == "win" || k == "windows" || k == "lwin" || k == "rwin") return VK_LWIN;
    if (k == "ctrl" || k == "control") return VK_CONTROL;
    if (k == "alt" || k == "menu") return VK_MENU;
    if (k == "shift") return VK_SHIFT;
    if (k == "enter" || k == "return") return VK_RETURN;
    if (k == "tab") return VK_TAB;
    if (k == "esc" || k == "escape") return VK_ESCAPE;
    if (k == "space") return VK_SPACE;
    if (k == "backspace") return VK_BACK;
    if (k == "delete" || k == "del") return VK_DELETE;
    if (k == "up") return VK_UP;
    if (k == "down") return VK_DOWN;
    if (k == "left") return VK_LEFT;
    if (k == "right") return VK_RIGHT;
    if (k == "f1") return VK_F1;
    if (k == "f2") return VK_F2;
    if (k == "f3") return VK_F3;
    if (k == "f4") return VK_F4;
    if (k == "f5") return VK_F5;
    if (k == "f6") return VK_F6;
    if (k == "f7") return VK_F7;
    if (k == "f8") return VK_F8;
    if (k == "f9") return VK_F9;
    if (k == "f10") return VK_F10;
    if (k == "f11") return VK_F11;
    if (k == "f12") return VK_F12;

    if (k.length() == 1) {
        SHORT s = VkKeyScanA(k[0]);
        if (s != -1) return (WORD)(s & 0xFF);
    }
    return 0;
}

bool DoSendHotkey(const std::vector<std::string>& keys) {
    if (keys.empty()) return false;
    std::vector<WORD> vkList;
    for (const auto& k : keys) {
        WORD vk = KeyNameToVk(k);
        if (vk != 0) vkList.push_back(vk);
    }
    if (vkList.empty()) return false;

    // Press in order
    for (WORD vk : vkList) {
        INPUT in = {};
        in.type = INPUT_KEYBOARD;
        in.ki.wVk = vk;
        SendInput(1, &in, sizeof(INPUT));
        std::this_thread::sleep_for(std::chrono::milliseconds(15));
    }

    std::this_thread::sleep_for(std::chrono::milliseconds(30));

    // Release in reverse order
    for (auto it = vkList.rbegin(); it != vkList.rend(); ++it) {
        INPUT in = {};
        in.type = INPUT_KEYBOARD;
        in.ki.wVk = *it;
        in.ki.dwFlags = KEYEVENTF_KEYUP;
        SendInput(1, &in, sizeof(INPUT));
        std::this_thread::sleep_for(std::chrono::milliseconds(15));
    }
    return true;
}

bool DoDragMouse(int fromX, int fromY, int toX, int toY, double durationSec) {
    int screenW = GetSystemMetrics(SM_CXSCREEN);
    int screenH = GetSystemMetrics(SM_CYSCREEN);
    if (screenW <= 0 || screenH <= 0) return false;

    DWORD normFromX = (DWORD)(((double)fromX * 65535.0) / (screenW - 1));
    DWORD normFromY = (DWORD)(((double)fromY * 65535.0) / (screenH - 1));

    INPUT in[2] = {};
    in[0].type = INPUT_MOUSE;
    in[0].mi.dx = normFromX;
    in[0].mi.dy = normFromY;
    in[0].mi.dwFlags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE;

    in[1].type = INPUT_MOUSE;
    in[1].mi.dwFlags = MOUSEEVENTF_LEFTDOWN;

    SendInput(2, in, sizeof(INPUT));

    int steps = 15;
    int stepDelay = (int)((durationSec * 1000.0) / steps);
    if (stepDelay < 10) stepDelay = 10;

    for (int i = 1; i <= steps; ++i) {
        double factor = (double)i / steps;
        int curX = (int)(fromX + factor * (toX - fromX));
        int curY = (int)(fromY + factor * (toY - fromY));

        DWORD normX = (DWORD)(((double)curX * 65535.0) / (screenW - 1));
        DWORD normY = (DWORD)(((double)curY * 65535.0) / (screenH - 1));

        INPUT move = {};
        move.type = INPUT_MOUSE;
        move.mi.dx = normX;
        move.mi.dy = normY;
        move.mi.dwFlags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE;
        SendInput(1, &move, sizeof(INPUT));
        std::this_thread::sleep_for(std::chrono::milliseconds(stepDelay));
    }

    INPUT up = {};
    up.type = INPUT_MOUSE;
    up.mi.dwFlags = MOUSEEVENTF_LEFTUP;
    SendInput(1, &up, sizeof(INPUT));
    return true;
}

void EnsureInputDesktop() {
    HDESK hInput = OpenInputDesktop(0, FALSE, GENERIC_ALL);
    if (hInput) {
        SetThreadDesktop(hInput);
        CloseDesktop(hInput);
    }
}

// ================= Main Dispatcher =================
int main() {
    // Enable UTF-8 console output
    SetConsoleOutputCP(CP_UTF8);

    EnsureInputDesktop();

    // Initialize GDI+
    Gdiplus::GdiplusStartupInput gdiplusStartupInput;
    Gdiplus::GdiplusStartup(&g_gdiplusToken, &gdiplusStartupInput, NULL);

    // Initialize COM as STA for UI Automation
    HRESULT hr = CoInitialize(NULL);

    hr = CoCreateInstance(CLSID_CUIAutomation, NULL, CLSCTX_INPROC_SERVER, IID_IUIAutomation, (void**)&g_pAutomation);

    std::string line;
    while (std::getline(std::cin, line)) {
        if (line.empty()) continue;

        EnsureInputDesktop();

        std::string action = ExtractParam(line, "action");
        int reqId = ExtractIntParam(line, "id", 0);

        if (action == "ping") {
            OutputSuccess(reqId, "{\"status\":\"pong\",\"engine\":\"jarvis-os-bridge (Native Win32/C++)\"}");
            continue;
        }

        // 1. CAPTURE_SCREEN & CAPTURE_SCREEN_RAM (100% In-RAM, Zero SSD writes)
        if (action == "capture_screen" || action == "capture_screen_ram") {
            std::string base64Data;
            int width = 0, height = 0;
            if (CaptureScreenRam(base64Data, width, height)) {
                // Extract active foreground window title (UTF-8 safe)
                HWND hwndFg = GetForegroundWindow();
                std::string fgTitleUtf8 = "";
                if (hwndFg) {
                    WCHAR wTitle[512] = {0};
                    GetWindowTextW(hwndFg, wTitle, 512);
                    fgTitleUtf8 = WideToUtf8(wTitle);
                }

                // Extract system clock and date formatted from Windows local time (UTF-8 safe)
                SYSTEMTIME st;
                GetLocalTime(&st);
                WCHAR wTime[64] = {0};
                WCHAR wDate[64] = {0};
                GetTimeFormatW(LOCALE_USER_DEFAULT, 0, &st, NULL, wTime, 64);
                GetDateFormatW(LOCALE_USER_DEFAULT, DATE_LONGDATE, &st, NULL, wDate, 64);
                std::string timeStr = WideToUtf8(wTime);
                std::string dateStr = WideToUtf8(wDate);

                std::ostringstream json;
                json << "{"
                     << "\"image_name\":\"current_screen.png\","
                     << "\"status\":\"success\","
                     << "\"method\":\"GDI+ In-RAM (Zero Disk I/O)\","
                     << "\"width\":" << width << ","
                     << "\"height\":" << height << ","
                     << "\"active_window\":\"" << EscapeJson(fgTitleUtf8) << "\","
                     << "\"system_clock\":\"" << EscapeJson(timeStr + ", " + dateStr) << "\","
                     << "\"coordinate_system\":\"normalized\","
                     << "\"origin\":\"bottom-left (0.0, 0.0)\","
                     << "\"top_right\":\"(1.0, 1.0)\"";
                if (action == "capture_screen_ram") {
                    json << ",\"base64\":\"" << base64Data << "\"";
                }
                json << "}";
                OutputSuccess(reqId, json.str());
            } else {
                OutputError(reqId,
                            "RamCaptureFailed",
                            "Failed to capture desktop screenshot into RAM.",
                            "GDI or GDI+ memory stream returned an error.",
                            "Ensure the application has desktop interaction permissions.");
            }
            continue;
        }

        // 2. GET_SCREEN_STATE (Dual Scan: Target 1 Active Window / Desktop + Target 2 Taskbar)
        if (action == "get_screen_state") {
            if (!g_pAutomation) {
                OutputError(reqId,
                            "UiaScanFailed",
                            "UI Automation COM interface not initialized.",
                            "CoCreateInstance(CLSID_CUIAutomation) failed.",
                            "Restart the agent or check Windows UI Automation service.");
                continue;
            }

            g_tagMap.clear();

            // Detect Target 1: Foreground Window vs Desktop
            HWND hwndForeground = GetForegroundWindow();
            bool isDesktop = false;
            std::string target1Title = "Desktop (No active applications opened)";

            if (hwndForeground == NULL || hwndForeground == GetDesktopWindow()) {
                isDesktop = true;
            } else {
                WCHAR className[256] = {0};
                GetClassNameW(hwndForeground, className, 256);
                if (wcscmp(className, L"Progman") == 0 || wcscmp(className, L"WorkerW") == 0 || wcscmp(className, L"Shell_TrayWnd") == 0) {
                    isDesktop = true;
                } else {
                    WCHAR wTitle[512] = {0};
                    GetWindowTextW(hwndForeground, wTitle, 512);
                    target1Title = WideToUtf8(wTitle);
                    if (target1Title.empty()) target1Title = "Active Window (" + WideToUtf8(className) + ")";
                }
            }

            std::vector<CachedTag> target1Tags;
            int tagCounter = 0;

            std::string target1Label = isDesktop ? "Target 1 (Desktop)" : ("Target 1 (" + target1Title + ")");

            if (isDesktop) {
                HWND hwndProgman = FindWindowW(L"Progman", NULL);
                if (!hwndProgman) hwndProgman = GetDesktopWindow();
                IUIAutomationElement* pDesktopElem = nullptr;
                if (SUCCEEDED(g_pAutomation->ElementFromHandle(hwndProgman, &pDesktopElem)) && pDesktopElem) {
                    CollectUiaElements(g_pAutomation, pDesktopElem, target1Label, tagCounter, 40, true, target1Tags);
                    pDesktopElem->Release();
                }
            } else {
                IUIAutomationElement* pFgElem = nullptr;
                if (SUCCEEDED(g_pAutomation->ElementFromHandle(hwndForeground, &pFgElem)) && pFgElem) {
                    CollectUiaElements(g_pAutomation, pFgElem, target1Label, tagCounter, 55, true, target1Tags);
                    pFgElem->Release();
                }
            }

            int target1EndTag = tagCounter;

            // Target 2: Windows Taskbar (continues with the same unified tagCounter sequence)
            HWND hwndTaskbar = FindWindowW(L"Shell_TrayWnd", NULL);
            std::vector<CachedTag> taskbarTags;
            if (hwndTaskbar) {
                IUIAutomationElement* pTbElem = nullptr;
                if (SUCCEEDED(g_pAutomation->ElementFromHandle(hwndTaskbar, &pTbElem)) && pTbElem) {
                    CollectUiaElements(g_pAutomation, pTbElem, "Target 2 (Taskbar)", tagCounter, 60, false, taskbarTags);
                    pTbElem->Release();
                }
            }

            int taskbarStartTag = target1EndTag + 1;
            int taskbarEndTag = tagCounter;

            // Build Output Text & JSON
            std::ostringstream textOut;
            if (isDesktop) {
                textOut << "================ TARGET 1: DESKTOP (No active application opened)";
            } else {
                textOut << "================ TARGET 1: ACTIVE WINDOW (" << target1Title << ")";
            }

            if (target1Tags.empty()) {
                textOut << " [No interactive elements] ================\n";
                textOut << "(No interactive child elements discovered in Target 1)\n";
            } else {
                textOut << " [Tags 1 to " << target1EndTag << "] ================\n";
                for (const auto& tag : target1Tags) {
                    textOut << "[" << tag.tagId << "] " << tag.controlType << ": \"" << tag.name << "\" "
                            << "[rect: " << tag.rect.left << ", " << tag.rect.top << ", "
                            << (tag.rect.right - tag.rect.left) << ", " << (tag.rect.bottom - tag.rect.top) << "]\n";
                }
            }

            textOut << "\n================ TARGET 2: WINDOWS TASKBAR (Pinned & Running Apps)";
            if (taskbarTags.empty()) {
                textOut << " [No elements] ================\n";
                textOut << "(Taskbar scanned, no buttons or items enumerated)\n";
            } else {
                textOut << " [Tags " << taskbarStartTag << " to " << taskbarEndTag << "] ================\n";
                for (const auto& tag : taskbarTags) {
                    textOut << "[" << tag.tagId << "] Taskbar " << tag.controlType << ": \"" << tag.name << "\" "
                            << "[rect: " << tag.rect.left << ", " << tag.rect.top << ", "
                            << (tag.rect.right - tag.rect.left) << ", " << (tag.rect.bottom - tag.rect.top) << "]\n";
                }
            }

            std::ostringstream jsonOut;
            jsonOut << "{"
                    << "\"is_desktop\":" << (isDesktop ? "true" : "false") << ","
                    << "\"target1_title\":\"" << EscapeJson(target1Title) << "\","
                    << "\"target1_elements_count\":" << target1Tags.size() << ","
                    << "\"target1_start_tag\":" << (target1Tags.empty() ? 0 : 1) << ","
                    << "\"target1_end_tag\":" << target1EndTag << ","
                    << "\"taskbar_elements_count\":" << taskbarTags.size() << ","
                    << "\"taskbar_start_tag\":" << (taskbarTags.empty() ? 0 : taskbarStartTag) << ","
                    << "\"taskbar_end_tag\":" << taskbarEndTag << ","
                    << "\"screen_text\":\"" << EscapeJson(textOut.str()) << "\""
                    << "}";
            OutputSuccess(reqId, jsonOut.str());
            continue;
        }

        // 3. CLICK_COORDINATES
        if (action == "click_coordinates") {
            double x = ExtractDoubleParam(line, "x", -1.0);
            double y = ExtractDoubleParam(line, "y", -1.0);
            bool isDouble = ExtractBoolParam(line, "double", false);
            bool isRight = ExtractBoolParam(line, "right", false);

            if (x < 0.0 || y < 0.0) {
                OutputError(reqId, "InvalidParameters", "Invalid x, y coordinates provided.", "Coordinates must be non-negative numbers.", "Specify normalized coordinates (0.0=bottom/left to 1.0=top/right, e.g. x: 0.5, y: 0.5).");
                continue;
            }

            int pixelX = 0, pixelY = 0;
            NormalizeToPixel(x, y, pixelX, pixelY);

            if (DoClickCoordinates(pixelX, pixelY, isDouble, isRight)) {
                std::ostringstream msg;
                msg << "{\"status\":\"success\",\"message\":\"Clicked at normalized ("
                    << x << ", " << y << ") -> screen pixel (" << pixelX << ", " << pixelY << ")\"}";
                OutputSuccess(reqId, msg.str());
            } else {
                OutputError(reqId, "InputInjectionBlocked", "Failed to inject mouse click via SendInput.", "SendInput was blocked or returned zero.", "Check if target window is elevated or running as administrator.");
            }
            continue;
        }

        // 4. CLICK_ELEMENT
        if (action == "click_element") {
            std::string tagId = ExtractParam(line, "tag_id");
            if (tagId.empty()) {
                int intTag = ExtractIntParam(line, "tag_id", -1);
                if (intTag >= 0) tagId = std::to_string(intTag);
            }

            // Normalization: strip accidental '#' or 'T' prefixes if LLM outputted e.g. "T18" or "#18"
            if (!tagId.empty() && (tagId[0] == 'T' || tagId[0] == 't' || tagId[0] == '#')) {
                std::string stripped = tagId.substr(1);
                if (!stripped.empty() && isdigit(stripped[0])) {
                    if (g_tagMap.find(stripped) != g_tagMap.end()) {
                        tagId = stripped;
                    }
                }
            }

            if (tagId.empty()) {
                OutputError(reqId, "InvalidParameters", "Missing 'tag_id' parameter.", "Parameter 'tag_id' was not found.", "Provide a valid numeric tag ID from get_screen_state (e.g. tag_id: 1).");
                continue;
            }

            auto it = g_tagMap.find(tagId);
            if (it == g_tagMap.end()) {
                OutputError(reqId, "TagIdNotFound", "Tag ID [" + tagId + "] not found in current screen state.", "The tag was not enumerated or screen state has changed.", "Call get_screen_state to refresh element tag IDs.");
                continue;
            }

            RECT r = it->second.rect;
            int centerX = (r.left + r.right) / 2;
            int centerY = (r.top + r.bottom) / 2;
            bool isDouble = ExtractBoolParam(line, "double", false);
            bool isRight = ExtractBoolParam(line, "right", false);

            int screenW = GetSystemMetrics(SM_CXSCREEN);
            int screenH = GetSystemMetrics(SM_CYSCREEN);
            if (screenW <= 0) screenW = 1920;
            if (screenH <= 0) screenH = 1080;
            double normX = (double)centerX / (double)(screenW - 1);
            double normY = 1.0 - ((double)centerY / (double)(screenH - 1));

            if (DoClickCoordinates(centerX, centerY, isDouble, isRight)) {
                std::ostringstream json;
                json << "{\"status\":\"success\","
                     << "\"tag_id\":\"" << EscapeJson(tagId) << "\","
                     << "\"target\":\"" << EscapeJson(it->second.targetSource) << "\","
                     << "\"control\":\"" << EscapeJson(it->second.controlType) << "\","
                     << "\"name\":\"" << EscapeJson(it->second.name) << "\","
                     << "\"clicked_x\":" << centerX << ","
                     << "\"clicked_y\":" << centerY << ","
                     << "\"norm_x\":" << normX << ","
                     << "\"norm_y\":" << normY << "}";
                OutputSuccess(reqId, json.str());
            } else {
                OutputError(reqId, "InputInjectionBlocked", "Failed to click element via SendInput.", "SendInput blocked.", "Check window elevation.");
            }
            continue;
        }

        // 5. TYPE_TEXT
        if (action == "type_text") {
            std::string text = ExtractParam(line, "text");
            bool pressEnter = ExtractBoolParam(line, "press_enter", false);
            double x = ExtractDoubleParam(line, "x", -1.0);
            double y = ExtractDoubleParam(line, "y", -1.0);

            if (text.empty() && !pressEnter) {
                OutputError(reqId, "InvalidParameters", "Missing 'text' parameter for type_text.", "Parameter 'text' was not provided.", "Provide text to type (e.g. text: \"edge\", press_enter: true).");
                continue;
            }

            int pixelX = -1, pixelY = -1;
            if (x >= 0.0 && y >= 0.0) {
                NormalizeToPixel(x, y, pixelX, pixelY);
            }

            if (DoTypeText(text, pressEnter, pixelX, pixelY)) {
                OutputSuccess(reqId, "{\"status\":\"success\",\"message\":\"Text typed successfully\"}");
            } else {
                OutputError(reqId, "InputInjectionBlocked", "Failed to inject keystrokes via SendInput.", "SendInput returned error.", "Verify window focus.");
            }
            continue;
        }

        // 6. SEND_HOTKEY
        if (action == "send_hotkey") {
            std::vector<std::string> keys = ExtractStringList(line, "keys");
            if (keys.empty()) {
                std::string singleKey = ExtractParam(line, "key");
                if (!singleKey.empty()) keys.push_back(singleKey);
            }

            if (keys.empty()) {
                std::string keysStr = ExtractParam(line, "keys");
                if (!keysStr.empty()) {
                    keys.push_back(keysStr);
                } else {
                    // Smart default: If send_hotkey is called with empty keys, default to pressing the Windows key ("win")
                    keys.push_back("win");
                }
            }

            if (DoSendHotkey(keys)) {
                OutputSuccess(reqId, "{\"status\":\"success\",\"message\":\"Hotkey sent successfully\"}");
            } else {
                OutputError(reqId, "InputInjectionBlocked", "Failed to send hotkey sequence.", "Unrecognized key code or SendInput blocked.", "Check key names (e.g. 'win', 'ctrl', 'alt', 's').");
            }
            continue;
        }

        // 7. DRAG_MOUSE
        if (action == "drag_mouse") {
            double fromX = ExtractDoubleParam(line, "from_x", -1.0);
            double fromY = ExtractDoubleParam(line, "from_y", -1.0);
            double toX = ExtractDoubleParam(line, "to_x", -1.0);
            double toY = ExtractDoubleParam(line, "to_y", -1.0);
            std::string durStr = ExtractParam(line, "duration");
            double duration = durStr.empty() ? 0.3 : std::stod(durStr);

            if (fromX < 0.0 || fromY < 0.0 || toX < 0.0 || toY < 0.0) {
                OutputError(reqId, "InvalidParameters", "Invalid drag coordinates provided.", "Coordinates must be non-negative numbers.", "Specify normalized coordinates (0.0=bottom/left to 1.0=top/right).");
                continue;
            }

            int pixelFromX = 0, pixelFromY = 0, pixelToX = 0, pixelToY = 0;
            NormalizeToPixel(fromX, fromY, pixelFromX, pixelFromY);
            NormalizeToPixel(toX, toY, pixelToX, pixelToY);

            if (DoDragMouse(pixelFromX, pixelFromY, pixelToX, pixelToY, duration)) {
                std::ostringstream msg;
                msg << "{\"status\":\"success\",\"message\":\"Mouse dragged from normalized ("
                    << fromX << ", " << fromY << ") to (" << toX << ", " << toY
                    << ") -> pixels (" << pixelFromX << ", " << pixelFromY << ") to ("
                    << pixelToX << ", " << pixelToY << ")\"}";
                OutputSuccess(reqId, msg.str());
            } else {
                OutputError(reqId, "InputInjectionBlocked", "Failed to drag mouse.", "SendInput error.", "Check coordinates.");
            }
            continue;
        }

        // 8. WINDOW_CONTROL
        if (action == "window_control") {
            std::string winAction = ExtractParam(line, "action_type");
            if (winAction.empty()) winAction = ExtractParam(line, "window_action");

            HWND hwnd = GetForegroundWindow();
            if (!hwnd) {
                OutputError(reqId, "WindowNotFound", "No active window found.", "GetForegroundWindow returned null.", "Ensure an application window is open.");
                continue;
            }

            if (winAction == "minimize") {
                ShowWindow(hwnd, SW_MINIMIZE);
            } else if (winAction == "maximize") {
                ShowWindow(hwnd, SW_MAXIMIZE);
            } else if (winAction == "restore") {
                ShowWindow(hwnd, SW_RESTORE);
            } else if (winAction == "close") {
                PostMessage(hwnd, WM_CLOSE, 0, 0);
            } else {
                OutputError(reqId, "InvalidParameters", "Unrecognized window action: " + winAction, "Valid actions: minimize, maximize, restore, close.", "Specify valid window_action.");
                continue;
            }
            OutputSuccess(reqId, "{\"status\":\"success\",\"action\":\"" + winAction + "\"}");
            continue;
        }

        OutputError(reqId, "UnrecognizedAction", "Unrecognized bridge action: " + action, "The requested action is not implemented in jarvis-os-bridge.", "Check action name.");
    }

    if (g_pAutomation) g_pAutomation->Release();
    CoUninitialize();
    if (g_gdiplusToken) Gdiplus::GdiplusShutdown(g_gdiplusToken);
    return 0;
}
