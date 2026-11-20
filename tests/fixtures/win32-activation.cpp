// Owned hidden Qt window; use existing Qt runtimes, never copy business binaries.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>

template<class T> T symbol(HMODULE module, const char* name) {
    auto address = GetProcAddress(module, name);
    if (!address) throw std::runtime_error(name);
    return reinterpret_cast<T>(address);
}
struct alignas(16) Opaque { unsigned char bytes[4096]{}; };
using AppCtor = void(__thiscall*)(void*, int&, char**, int);
using WidgetCtor = void(__thiscall*)(void*, void*, unsigned);
using WinId = uintptr_t(__thiscall*)(void*);
using Utf16 = const wchar_t*(__thiscall*)(const void*);
static Utf16 qtUtf16 = nullptr;
static void qtMessage(int, const void*, const void* message) {
    std::wcerr << qtUtf16(message) << std::endl;
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 4) return 2;
        std::wstring root = argv[3], plugins = root + L"\\platforms";
        if (GetFileAttributesW(plugins.c_str()) == INVALID_FILE_ATTRIBUTES)
            plugins = root + L"\\..\\platforms";
        SetDllDirectoryW(root.c_str());
        SetEnvironmentVariableW(L"QT_QPA_PLATFORM_PLUGIN_PATH", plugins.c_str());
        SetEnvironmentVariableW(L"QT_QPA_PLATFORM", L"windows");
        auto core = LoadLibraryW((root + L"\\Qt5Core.dll").c_str());
        LoadLibraryW((root + L"\\Qt5Gui.dll").c_str());
        auto widgets = LoadLibraryW((root + L"\\Qt5Widgets.dll").c_str());
        auto platform = LoadLibraryW((plugins + L"\\qwindows.dll").c_str());
        if (!widgets || !platform) throw std::runtime_error("Could not load Qt runtime");
        qtUtf16 = symbol<Utf16>(core,
#ifdef _WIN64
            "?utf16@QString@@QEBAPEBGXZ"
#else
            "?utf16@QString@@QBEPBGXZ"
#endif
        );
        symbol<void*(*)(void*)>(core,
#ifdef _WIN64
            "?qInstallMessageHandler@@YAP6AXW4QtMsgType@@AEBVQMessageLogContext@@AEBVQString@@@ZP6AX012@Z@Z"
#else
            "?qInstallMessageHandler@@YAP6AXW4QtMsgType@@ABVQMessageLogContext@@ABVQString@@@ZP6AX012@Z@Z"
#endif
        )(reinterpret_cast<void*>(&qtMessage));
        struct QtString { void* data; } pluginDirectory{};
        std::wstring pluginParent = plugins + L"\\..";
        int size = WideCharToMultiByte(CP_UTF8, 0, pluginParent.data(), static_cast<int>(pluginParent.size()), nullptr, 0, nullptr, nullptr);
        std::string utf8(size, '\0');
        WideCharToMultiByte(CP_UTF8, 0, pluginParent.data(), static_cast<int>(pluginParent.size()), utf8.data(), size, nullptr, nullptr);
        symbol<QtString*(*)(QtString*, const char*, int)>(core,
#ifdef _WIN64
            "?fromUtf8@QString@@SA?AV1@PEBDH@Z"
#else
            "?fromUtf8@QString@@SA?AV1@PBDH@Z"
#endif
        )(&pluginDirectory, utf8.c_str(), -1);
        symbol<void(*)(const QtString&)>(core,
#ifdef _WIN64
            "?addLibraryPath@QCoreApplication@@SAXAEBVQString@@@Z"
#else
            "?addLibraryPath@QCoreApplication@@SAXABVQString@@@Z"
#endif
        )(pluginDirectory);
        std::cerr << "Qt runtime loaded" << std::endl;
        Opaque app, window;
        int appArgc = 1;
        char name[] = "xbot-owned-activation-fixture";
        char* appArgv[] = {name, nullptr};
#ifdef _WIN64
        const char* appSymbol = "??0QApplication@@QEAA@AEAHPEAPEADH@Z";
        const char* widgetSymbol = "??0QWidget@@QEAA@PEAV0@V?$QFlags@W4WindowType@Qt@@@@@Z";
        const char* winIdSymbol = "?winId@QWidget@@QEBA_KXZ";
        const char* architecture = "x64";
#else
        const char* appSymbol = "??0QApplication@@QAE@AAHPAPADH@Z";
        const char* widgetSymbol = "??0QWidget@@QAE@PAV0@V?$QFlags@W4WindowType@Qt@@@@@Z";
        const char* winIdSymbol = "?winId@QWidget@@QBEIXZ";
        const char* architecture = "x86";
#endif
        symbol<AppCtor>(widgets, appSymbol)(&app, appArgc, appArgv, 0x050f00);
        std::cerr << "QApplication constructed" << std::endl;
        symbol<WidgetCtor>(widgets, widgetSymbol)(&window, nullptr, 0);
        std::cerr << "QWidget constructed" << std::endl;
        auto hwnd = symbol<WinId>(widgets, winIdSymbol)(&window);
        SetWindowTextW(reinterpret_cast<HWND>(hwnd), argv[1]);
        FILETIME created{}, exited{}, kernel{}, user{};
        if (!GetProcessTimes(GetCurrentProcess(), &created, &exited, &kernel, &user)) return 3;
        auto instance = (uint64_t(created.dwHighDateTime) << 32) | created.dwLowDateTime;
        std::ofstream receipt(argv[2]);
        receipt << "{\"pid\":" << GetCurrentProcessId() << ",\"hwnd\":" << hwnd
            << ",\"architecture\":\"" << architecture << "\",\"creation_time\":\"" << instance << "\"}";
        receipt.close();
        symbol<int(*)()>(widgets, "?exec@QApplication@@SAHXZ")();
        ExitProcess(0);
    } catch (const std::exception& error) {
        std::cerr << error.what() << std::endl;
        return 4;
    }
}
