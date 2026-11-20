// Owned hidden Win32 fixture for the vendor misc.exe route, with no business UI.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
#include <fstream>
#include <string>

static bool suppressProbe = false;
LRESULT CALLBACK fixtureProc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
    if (msg == WM_DESTROY) { PostQuitMessage(0); return 0; }
    if (suppressProbe && msg == 0x2b66) return 0;
    return DefWindowProcW(hwnd, msg, wparam, lparam);
}
int wmain(int argc, wchar_t** argv) {
    if (argc != 3) return 2;
    suppressProbe = std::wstring(argv[2]) == L"unverified";
    WNDCLASSW cls{};
    cls.lpfnWndProc = fixtureProc;
    cls.hInstance = GetModuleHandleW(nullptr);
    cls.lpszClassName = L"WeWorkWindow";
    if (!RegisterClassW(&cls)) return 3;
    auto hwnd = CreateWindowExW(0, cls.lpszClassName, L"xbot-owned-misc-fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 400, 200, nullptr, nullptr, cls.hInstance, nullptr);
    if (!hwnd) return 4;
    if (std::wstring(argv[2]) == L"marker-only")
        SetPropA(hwnd, "RpaBugFixFlags", reinterpret_cast<HANDLE>(1));
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(GetCurrentProcess(), &created, &exited, &kernel, &user)) return 5;
    auto instance = (uint64_t(created.dwHighDateTime) << 32) | created.dwLowDateTime;
    std::ofstream receipt(argv[1]);
    receipt << "{\"pid\":" << GetCurrentProcessId() << ",\"hwnd\":" << reinterpret_cast<uintptr_t>(hwnd)
        << ",\"creation_time\":\"" << instance << "\"}";
    receipt.close();
    MSG message;
    while (GetMessageW(&message, nullptr, 0, 0) > 0) { TranslateMessage(&message); DispatchMessageW(&message); }
    return 0;
}
