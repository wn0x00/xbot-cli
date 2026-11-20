// Owned x86 DuiLib ABI fixture. Nothing is copied from a business executable.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
#include <fstream>
#include <string>

#ifdef BUILD_DLL
#ifdef GATED
static volatile unsigned char gates[3]{};
#endif
extern "C" __declspec(naked) LRESULT __stdcall FixtureOnGetObject(UINT, WPARAM, LPARAM, bool*) {
    __asm {
        push ebp
        mov ebp, esp
#ifdef GATED
        cmp byte ptr [gates], 0
        je declined
        cmp byte ptr [gates+1], 0
        je declined
        cmp byte ptr [gates+2], 0
        je declined
#endif
        cmp dword ptr [ebp+16], -4
        jne declined
        cmp dword ptr [ebp+12], 1
        jne declined
        mov eax, [ebp+20]
        mov byte ptr [eax], 1
        mov ecx, 0BAADF00Dh
        mov eax, 44332211h
        pop ebp
        ret 16
    declined:
        xor eax, eax
        mov ecx, 0BAADF00Dh
        pop ebp
        ret 16
    }
}
BOOL WINAPI DllMain(HINSTANCE, DWORD, LPVOID) { return TRUE; }
#else
struct Object { void** vtable; uint32_t cookie; };
extern "C" __declspec(naked) LRESULT __stdcall HandleMessage(UINT, WPARAM, LPARAM) {
    __asm {
        _emit 066h
        nop
        push ebp
        mov ebp, esp
        mov eax, [ecx+4]
        cmp dword ptr [ebp+12], 1
        jne finish
        xor eax, eax
    finish:
        pop ebp
        ret 12
    }
}
static Object object{};
static bool suppressResponse=false;
static LRESULT dispatch(UINT message, WPARAM wp, LPARAM lp) {
    auto method = object.vtable[9];
    auto self = &object;
    LRESULT result;
    __asm {
        mov ecx, self
        push lp
        push wp
        push message
        call method
        mov result, eax
    }
    return result;
}
LRESULT CALLBACK fixtureProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    if (msg==WM_DESTROY) { PostQuitMessage(0); return 0; }
    if (suppressResponse && msg==WM_GETOBJECT) return 0;
    if (msg==WM_GETOBJECT || msg==WM_APP+1) return dispatch(msg,wp,lp);
    return DefWindowProcW(hwnd,msg,wp,lp);
}
int wmain(int argc, wchar_t** argv) {
    if (argc!=3) return 2;
    suppressResponse=std::wstring(argv[2])==L"unverified";
    auto dll=LoadLibraryW(L"DuiLib.dll");
    if (!dll) return 3;
    auto onget=GetProcAddress(dll,"?OnGetObject@CWindowWnd@DuiLib@@MAEJIIJAA_N@Z");
    if (!onget) return 4;
    static void* table[20]{};
    table[9]=reinterpret_cast<void*>(&HandleMessage);
    table[14]=reinterpret_cast<void*>(onget);
    object.vtable=table;object.cookie=0x13579;
    WNDCLASSW cls{};cls.lpfnWndProc=fixtureProc;cls.hInstance=GetModuleHandleW(nullptr);cls.lpszClassName=L"WeWorkWindow";
    if (!RegisterClassW(&cls)) return 5;
    auto hwnd=CreateWindowExW(0,cls.lpszClassName,L"xbot-owned-wework-adapter",WS_OVERLAPPEDWINDOW,0,0,400,200,nullptr,nullptr,cls.hInstance,nullptr);
    if (!hwnd) return 6;
    if (std::wstring(argv[2])!=L"no-object") SetWindowLongW(hwnd,GWLP_USERDATA,reinterpret_cast<LONG>(&object));
    if (std::wstring(argv[2])==L"foreign-jump") {
        auto entry=reinterpret_cast<unsigned char*>(&HandleMessage);DWORD previous;
        if (!VirtualProtect(entry,5,PAGE_EXECUTE_READWRITE,&previous)) return 7;
        // Foreign E9 must never be accepted as our already-active implementation.
        entry[0]=0xe9;*reinterpret_cast<uint32_t*>(entry+1)=0;
        VirtualProtect(entry,5,previous,&previous);FlushInstructionCache(GetCurrentProcess(),entry,5);
    }
    FILETIME created{},exited{},kernel{},user{};GetProcessTimes(GetCurrentProcess(),&created,&exited,&kernel,&user);
    auto instance=(uint64_t(created.dwHighDateTime)<<32)|created.dwLowDateTime;
    std::ofstream receipt(argv[1]);receipt<<"{\"pid\":"<<GetCurrentProcessId()<<",\"hwnd\":"<<reinterpret_cast<uintptr_t>(hwnd)<<",\"creation_time\":\""<<instance<<"\"}";receipt.close();
    MSG msg;while(GetMessageW(&msg,nullptr,0,0)>0) { TranslateMessage(&msg);DispatchMessageW(&msg); }return 0;
}
#endif
