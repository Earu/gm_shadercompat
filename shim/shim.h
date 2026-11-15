#ifndef GM_SHADERCOMPAT_SHIM_H
#define GM_SHADERCOMPAT_SHIM_H

#include <cstdarg>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cctype>
#include <cmath>
#include <string>
#include <stdexcept>

typedef int8_t int8;
typedef uint8_t uint8;
typedef int16_t int16;
typedef uint16_t uint16;
typedef int32_t int32;
typedef uint32_t uint32;
typedef int64_t int64;
typedef uint64_t uint64;
typedef unsigned int uint;
typedef intptr_t intp;
typedef uintptr_t uintp;

#define DLL_EXPORT
#define DLL_IMPORT
#define DLL_GLOBAL_EXPORT
#define DLL_GLOBAL_IMPORT
#define DLL_CLASS_EXPORT
#define DLL_CLASS_IMPORT
#define PLATFORM_INTERFACE
#define FORCEINLINE inline

#ifndef _MSC_VER
#define __stdcall
#define __cdecl
#endif

typedef int BOOL;

#define ARRAYSIZE(a) (sizeof(a) / sizeof((a)[0]))
#define NOTE_UNUSED(x) ((void)(x))
#define MIN(a, b) (((a) < (b)) ? (a) : (b))
#define MAX(a, b) (((a) > (b)) ? (a) : (b))

#define DXABSTRACT_VS_PARAM_SLOTS 219
#define DXABSTRACT_VS_FIRST_BONE_SLOT 58
#define DXABSTRACT_VS_LAST_BONE_SLOT 216

enum _D3DFORMAT_shim {};
#define _D3DFORMAT _D3DFORMAT_shim

#define Assert(...) ((void)0)
#define AssertMsg(...) ((void)0)
#define AssertMsg1(...) ((void)0)
#define AssertMsg2(...) ((void)0)
#define AssertOnce(...) ((void)0)
#define Verify(x) (x)
#define DebuggerBreak() ((void)0)
#define DebuggerBreakIfDebugging() ((void)0)
#define Debugger() Error("Debugger() breakpoint (SIGTRAP in game)")

struct ToglError : std::runtime_error
{
	explicit ToglError(const std::string &message) : std::runtime_error(message) {}
};

inline void Error(const char *fmt, ...)
{
	char buf[1024];
	va_list args;
	va_start(args, fmt);
	vsnprintf(buf, sizeof(buf), fmt, args);
	va_end(args);
	throw ToglError(buf);
}

inline void Warning(const char *, ...) {}
inline void Msg(const char *, ...) {}
inline void DevMsg(const char *, ...) {}
inline void Plat_DebugString(const char *) {}
inline void GLMDebugPrintf(const char *, ...) {}

inline int V_vsnprintf(char *dest, int maxLen, const char *fmt, va_list args)
{
	int n = vsnprintf(dest, maxLen, fmt, args);
	if (maxLen > 0 && (n < 0 || n >= maxLen))
		dest[maxLen - 1] = 0;
	return n;
}

inline int V_snprintf(char *dest, int maxLen, const char *fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	int n = V_vsnprintf(dest, maxLen, fmt, args);
	va_end(args);
	return n;
}

inline int V_strlen(const char *s) { return (int)strlen(s); }
inline int V_strcmp(const char *a, const char *b) { return strcmp(a, b); }
inline int V_strncmp(const char *a, const char *b, int n) { return strncmp(a, b, n); }

inline int V_stricmp(const char *a, const char *b)
{
	for (;; a++, b++)
	{
		int ca = tolower((unsigned char)*a);
		int cb = tolower((unsigned char)*b);
		if (ca != cb || !ca)
			return ca - cb;
	}
}

inline const char *V_stristr(const char *haystack, const char *needle)
{
	size_t n = strlen(needle);
	for (; *haystack; haystack++)
	{
		size_t i = 0;
		while (i < n && tolower((unsigned char)haystack[i]) == tolower((unsigned char)needle[i]))
			i++;
		if (i == n)
			return haystack;
	}
	return nullptr;
}

inline void V_strncpy(char *dest, const char *src, int maxLen)
{
	if (maxLen <= 0)
		return;
	strncpy(dest, src, maxLen);
	dest[maxLen - 1] = 0;
}

inline void V_strcpy(char *dest, const char *src) { strcpy(dest, src); }

extern size_t g_ToglTruncations;
extern size_t g_ToglTruncatedSize;

inline char *V_strncat(char *dest, const char *src, size_t destSize, int maxCharsToCopy = -1)
{
	size_t len = strlen(dest);
	size_t srcLen = strlen(src);
	if (maxCharsToCopy >= 0 && (size_t)maxCharsToCopy < srcLen)
		srcLen = maxCharsToCopy;
	if (len + srcLen + 1 > destSize)
	{
		if (g_ToglTruncations == 0)
			g_ToglTruncatedSize = destSize;
		g_ToglTruncations++;
		srcLen = destSize > len + 1 ? destSize - len - 1 : 0;
	}
	memcpy(dest + len, src, srcLen);
	dest[len + srcLen] = 0;
	return dest;
}

inline char *V_strcat(char *dest, const char *src, size_t destSize) { return V_strncat(dest, src, destSize); }

inline float RandomFloat(float lo, float hi) { return lo + (hi - lo) * 0.5f; }

enum GLMDecodeKind { eD3D_SIO };
inline const char *GLMDecode(GLMDecodeKind, unsigned long) { return ""; }

class CUtlBuffer
{
public:
	enum BufferFlags_t { TEXT_BUFFER = 0x1 };

	CUtlBuffer(int growSize = 0, int initSize = 0, int flags = 0)
	{
		(void)growSize;
		(void)flags;
		m_size = 0;
		m_data = nullptr;
		EnsureCapacity(initSize);
	}

	~CUtlBuffer() { free(m_data); }

	CUtlBuffer(const CUtlBuffer &) = delete;
	CUtlBuffer &operator=(const CUtlBuffer &) = delete;

	void EnsureCapacity(int size)
	{
		if (size <= m_size)
			return;
		m_data = (char *)realloc(m_data, size + 1);
		memset(m_data + m_size, 0, size + 1 - m_size);
		m_size = size;
	}

	void *Base() { return m_data; }
	const void *Base() const { return m_data; }
	int Size() const { return m_size; }

	void PutString(const char *s) { V_strcat(m_data, s, m_size); }

private:
	char *m_data;
	int m_size;
};

class CUtlString
{
public:
	CUtlString() {}
	CUtlString(const char *s) : m_str(s ? s : "") {}
	CUtlString(const CUtlString &) = default;
	CUtlString &operator=(const CUtlString &) = default;
	CUtlString &operator=(const char *s)
	{
		m_str = s ? s : "";
		return *this;
	}

	const char *Get() const { return m_str.c_str(); }
	const char *String() const { return m_str.c_str(); }
	operator const char *() const { return m_str.c_str(); }
	int Length() const { return (int)m_str.size(); }
	bool IsEmpty() const { return m_str.empty(); }
	void Set(const char *s) { m_str = s ? s : ""; }

	CUtlString &operator+=(const CUtlString &other)
	{
		m_str += other.m_str;
		return *this;
	}
	CUtlString &operator+=(const char *s)
	{
		m_str += s;
		return *this;
	}
	CUtlString &operator+=(char c)
	{
		m_str += c;
		return *this;
	}

	friend CUtlString operator+(const CUtlString &a, const CUtlString &b)
	{
		CUtlString r(a);
		r += b;
		return r;
	}
	friend CUtlString operator+(const CUtlString &a, const char *b)
	{
		CUtlString r(a);
		r += b;
		return r;
	}
	friend CUtlString operator+(const char *a, const CUtlString &b)
	{
		CUtlString r(a);
		r += b;
		return r;
	}

	bool operator==(const CUtlString &o) const { return m_str == o.m_str; }
	bool operator==(const char *o) const { return m_str == (o ? o : ""); }
	bool operator!=(const CUtlString &o) const { return m_str != o.m_str; }
	bool operator!=(const char *o) const { return !(*this == o); }

private:
	std::string m_str;
};

#endif
