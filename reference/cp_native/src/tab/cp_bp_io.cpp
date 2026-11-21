// cp_bp_io.cpp -- voir cp_bp_io.h.
#include "cp_bp_io.h"
#include "cp_bp_model.h"

#include <cstdio>
#include <cstring>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#endif

namespace cp { namespace bp {

namespace {

#ifdef _WIN32
// Les chemins viennent de REAPER en UTF-8 : un nom de projet accentue ne
// s'ouvre pas par fopen (ANSI). La regle de la suite (le depot, lot 3).
std::wstring wide(const std::string& s)
{
    if (s.empty()) return std::wstring();
    const int n = MultiByteToWideChar(CP_UTF8, 0, s.c_str(), (int)s.size(), nullptr, 0);
    std::wstring w((size_t)n, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s.c_str(), (int)s.size(), &w[0], n);
    return w;
}
FILE* open_file(const std::string& path, const char* mode)
{
    const std::wstring m = wide(mode);
    return _wfopen(wide(path).c_str(), m.c_str());
}
bool replace_file(const std::string& from, const std::string& to)
{
    return MoveFileExW(wide(from).c_str(), wide(to).c_str(),
                       MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) != 0;
}
#else
FILE* open_file(const std::string& path, const char* mode) { return std::fopen(path.c_str(), mode); }
bool replace_file(const std::string& from, const std::string& to)
{
    return std::rename(from.c_str(), to.c_str()) == 0;
}
#endif

void put_u32(std::vector<unsigned char>& b, unsigned v)
{
    for (int i = 0; i < 4; ++i) b.push_back((unsigned char)((v >> (8 * i)) & 0xff));
}
void put_f32(std::vector<unsigned char>& b, const float* x, size_t n)
{
    const size_t at = b.size();
    b.resize(at + n * 4);
    std::memcpy(&b[at], x, n * 4);
}
unsigned get_u32(const unsigned char* p)
{
    return (unsigned)p[0] | ((unsigned)p[1] << 8) | ((unsigned)p[2] << 16) | ((unsigned)p[3] << 24);
}

bool slurp(const std::string& path, std::vector<unsigned char>& out)
{
    FILE* f = open_file(path, "rb");
    if (!f) return false;
    std::fseek(f, 0, SEEK_END);
    const long sz = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    if (sz < 0) { std::fclose(f); return false; }
    out.resize((size_t)sz);
    const size_t got = sz > 0 ? std::fread(out.data(), 1, (size_t)sz, f) : 0;
    std::fclose(f);
    return got == (size_t)sz;
}

} // namespace

bool write_atomic(const std::string& path, const void* data, size_t n)
{
    const std::string part = path + ".part";
    FILE* f = open_file(part, "wb");
    if (!f) return false;
    const size_t put = n ? std::fwrite(data, 1, n, f) : 0;
    const bool ok = (put == n) && std::fclose(f) == 0;
    if (!ok) { std::remove(part.c_str()); return false; }
    return replace_file(part, path);
}

bool read_audio(const std::string& path, std::vector<float>& out, std::string& err)
{
    std::vector<unsigned char> b;
    if (!slurp(path, b)) { err = "cannot read " + path; return false; }
    if (b.size() < 12 || std::memcmp(b.data(), "CPA1", 4) != 0) { err = "not a CPA1 file"; return false; }
    const unsigned sr = get_u32(&b[4]), n = get_u32(&b[8]);
    if (sr != (unsigned)kSr) { err = "sample rate must be 22050"; return false; }
    if (b.size() < 12 + (size_t)n * 4) { err = "truncated audio"; return false; }
    out.resize(n);
    if (n) std::memcpy(out.data(), &b[12], (size_t)n * 4);
    return true;
}

bool write_audio(const std::string& path, const float* x, size_t n)
{
    std::vector<unsigned char> b;
    b.insert(b.end(), { 'C', 'P', 'A', '1' });
    put_u32(b, (unsigned)kSr);
    put_u32(b, (unsigned)n);
    put_f32(b, x, n);
    return write_atomic(path, b.data(), b.size());
}

bool write_cache(const std::string& path, const Posteriors& p)
{
    std::vector<unsigned char> b;
    b.insert(b.end(), { 'C', 'P', 'B', '1' });
    put_u32(b, (unsigned)p.frames);
    put_f32(b, p.note.data(), p.note.size());
    put_f32(b, p.onset.data(), p.onset.size());
    put_f32(b, p.contour.data(), p.contour.size());
    return write_atomic(path, b.data(), b.size());
}

bool read_cache(const std::string& path, Posteriors& p, std::string& err)
{
    std::vector<unsigned char> b;
    if (!slurp(path, b)) { err = "cannot read " + path; return false; }
    if (b.size() < 8 || std::memcmp(b.data(), "CPB1", 4) != 0) { err = "not a CPB1 file"; return false; }
    const unsigned F = get_u32(&b[4]);
    const size_t need = 8 + (size_t)F * (kNotes * 2 + kContour) * 4;
    if (b.size() != need) { err = "truncated cache"; return false; }
    p.frames = (int)F;
    p.note.resize((size_t)F * kNotes);
    p.onset.resize((size_t)F * kNotes);
    p.contour.resize((size_t)F * kContour);
    size_t at = 8;
    std::memcpy(p.note.data(), &b[at], p.note.size() * 4);     at += p.note.size() * 4;
    std::memcpy(p.onset.data(), &b[at], p.onset.size() * 4);   at += p.onset.size() * 4;
    std::memcpy(p.contour.data(), &b[at], p.contour.size() * 4);
    return true;
}

bool write_notes(const std::string& path, const Posteriors& p, const std::vector<Note>& notes)
{
    std::string s;
    char line[128];
    s += "CPTAB 1\n";
    std::snprintf(line, sizeof line, "frames %d hop %d sr %d\n", p.frames, kHop, kSr);
    s += line;
    for (const Note& n : notes) {
        std::snprintf(line, sizeof line, "%d %d %d %.4f %.4f %d", n.start, n.end, n.pitch,
                      n.amp, n.peak, (int)n.bends.size());
        s += line;
        for (int b : n.bends) {
            std::snprintf(line, sizeof line, " %d", b);
            s += line;
        }
        s += '\n';
    }
    return write_atomic(path, s.data(), s.size());
}

}} // namespace cp::bp

