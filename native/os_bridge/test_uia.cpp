#include <windows.h>
#include <uiautomation.h>
#include <iostream>

#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "oleaut32.lib")
#pragma comment(lib, "user32.lib")

int main() {
    HDESK hInputDesk = OpenInputDesktop(0, FALSE, GENERIC_ALL);
    if (hInputDesk) {
        SetThreadDesktop(hInputDesk);
    }

    CoInitialize(NULL);
    IUIAutomation* pAutomation = NULL;
    CoCreateInstance(CLSID_CUIAutomation, NULL, CLSCTX_INPROC_SERVER, IID_IUIAutomation, (void**)&pAutomation);

    HWND hwndTaskbar = FindWindowW(L"Shell_TrayWnd", NULL);
    std::cout << "Shell_TrayWnd HWND=" << hwndTaskbar << std::endl;

    if (hwndTaskbar) {
        IUIAutomationElement* pTbElem = NULL;
        pAutomation->ElementFromHandle(hwndTaskbar, &pTbElem);
        if (pTbElem) {
            IUIAutomationCondition* pTrue = NULL;
            pAutomation->CreateTrueCondition(&pTrue);
            IUIAutomationElementArray* pArray = NULL;
            pTbElem->FindAll(TreeScope_Descendants, pTrue, &pArray);
            if (pArray) {
                int len = 0;
                pArray->get_Length(&len);
                std::cout << "Taskbar Descendants Count: " << len << std::endl;
                for (int i = 0; i < len; ++i) {
                    IUIAutomationElement* pChild = NULL;
                    pArray->GetElement(i, &pChild);
                    if (pChild) {
                        BSTR name = NULL;
                        pChild->get_CurrentName(&name);
                        CONTROLTYPEID ct = 0;
                        pChild->get_CurrentControlType(&ct);
                        RECT r = {0};
                        pChild->get_CurrentBoundingRectangle(&r);
                        if (name && wcslen(name) > 0) {
                            std::wcout << L"  [" << i << L"] (ct=" << ct << L") Name: \"" << name << L"\" Rect: ["
                                       << r.left << L"," << r.top << L"," << (r.right - r.left) << L"," << (r.bottom - r.top) << L"]" << std::endl;
                        }
                        if (name) SysFreeString(name);
                        pChild->Release();
                    }
                }
                pArray->Release();
            }
            if (pTrue) pTrue->Release();
            pTbElem->Release();
        }
    }

    // Now test Foreground Window or Desktop
    HWND hwndActive = GetForegroundWindow();
    std::cout << "Foreground Window HWND=" << hwndActive << std::endl;
    if (hwndActive) {
        WCHAR title[256] = {0};
        GetWindowTextW(hwndActive, title, 256);
        std::wcout << L"Foreground Title: \"" << title << L"\"" << std::endl;
    }

    CoUninitialize();
    if (hInputDesk) CloseDesktop(hInputDesk);
    return 0;
}
