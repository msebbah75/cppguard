// example.cpp — test file for cpp-complexity / cpp-guard
// Covers: if/else, loops, switch, ternary, logical ops,
//         nested lambdas, templates, try/catch, recursion

#include <vector>
#include <string>
#include <stdexcept>

// ─────────────────────────────────────────────────────────────
// 1. TRIVIAL — CC=1, Cognitive=0
// ─────────────────────────────────────────────────────────────
int add(int a, int b) {
    return a + b;
}

// ─────────────────────────────────────────────────────────────
// 2. MODERATE — CC≈5, several branches
// ─────────────────────────────────────────────────────────────
std::string classify_temperature(double celsius) {
    if (celsius < 0.0) {
        return "freezing";
    } else if (celsius < 10.0) {
        return "cold";
    } else if (celsius < 20.0) {
        return "mild";
    } else if (celsius < 30.0) {
        return "warm";
    } else {
        return "hot";
    }
}

// ─────────────────────────────────────────────────────────────
// 3. SWITCH — each case adds to CC
// ─────────────────────────────────────────────────────────────
int days_in_month(int month, int year) {
    switch (month) {
        case 1: case 3: case 5: case 7:
        case 8: case 10: case 12:
            return 31;
        case 4: case 6: case 9: case 11:
            return 30;
        case 2:
            // Ternary + logical operators — extra CC hits
            return ((year % 4 == 0 && year % 100 != 0) || year % 400 == 0)
                   ? 29 : 28;
        default:
            throw std::invalid_argument("Invalid month");
    }
}

// ─────────────────────────────────────────────────────────────
// 4. LOOPS — for, while, do-while, range-for
// ─────────────────────────────────────────────────────────────
int sum_evens(const std::vector<int>& v) {
    int total = 0;
    for (int x : v) {          // +1
        if (x % 2 == 0) {      // +1
            total += x;
        }
    }
    return total;
}

int collatz_steps(int n) {
    int steps = 0;
    while (n != 1) {           // +1
        if (n % 2 == 0) {      // +1
            n /= 2;
        } else {
            n = 3 * n + 1;
        }
        ++steps;
        if (steps > 10000) break; // +1  (safety valve)
    }
    return steps;
}

// ─────────────────────────────────────────────────────────────
// 5. RECURSION + deep nesting — HIGH complexity
// ─────────────────────────────────────────────────────────────
long long fibonacci(int n) {
    if (n <= 0) return 0;      // +1
    if (n == 1) return 1;      // +1
    return fibonacci(n - 1) + fibonacci(n - 2);
}

// ─────────────────────────────────────────────────────────────
// 6. DELIBERATELY COMPLEX — should trigger CC and Cognitive
//    warnings at default thresholds (CC>10, Cog>15)
// ─────────────────────────────────────────────────────────────
int parse_token(const std::string& s, int pos, bool strict) {
    int result = 0;

    if (s.empty() || pos < 0 || pos >= (int)s.size()) { // +1 +1 +1
        return -1;
    }

    while (pos < (int)s.size()) {      // +1
        char c = s[pos];

        if (c >= '0' && c <= '9') {    // +1 +1 (&&)
            result = result * 10 + (c - '0');
            ++pos;
        } else if (c == '_' || c == '-') { // +1 +1 (||)
            if (strict) {              // +1  (nesting +1 cog)
                return -1;
            }
            ++pos;
        } else if (c == ' ' || c == '\t') { // +1 +1
            // skip whitespace
            do {                       // +1
                ++pos;
            } while (pos < (int)s.size() &&
                     (s[pos] == ' ' || s[pos] == '\t')); // +1 +1
        } else {
            if (strict && result == 0) { // +1 +1
                throw std::runtime_error("unexpected char");
            }
            break;
        }
    }

    return result;
}

// ─────────────────────────────────────────────────────────────
// 7. TRY / CATCH — each catch block adds CC
// ─────────────────────────────────────────────────────────────
double safe_divide(double a, double b) {
    try {
        if (b == 0.0) {                // +1
            throw std::domain_error("division by zero");
        }
        return a / b;
    } catch (const std::domain_error& e) {  // +1
        return 0.0;
    } catch (const std::exception& e) {     // +1
        return -1.0;
    } catch (...) {                         // +1
        return -2.0;
    }
}

// ─────────────────────────────────────────────────────────────
// 8. LAMBDA — nested function scope, Halstead operand-rich
// ─────────────────────────────────────────────────────────────
std::vector<int> filter_and_transform(const std::vector<int>& input,
                                       int threshold) {
    std::vector<int> output;
    output.reserve(input.size());

    auto process = [&](int val) -> int {
        if (val < 0) return 0;         // +1 inside lambda
        if (val > threshold) {         // +1
            return val * 2;
        }
        return val + threshold / 2;
    };

    for (int v : input) {             // +1
        int r = process(v);
        if (r != 0) {                 // +1
            output.push_back(r);
        }
    }
    return output;
}

// ─────────────────────────────────────────────────────────────
// 9. CLASS METHOD with high parameter count
// ─────────────────────────────────────────────────────────────
class Renderer {
public:
    // Many parameters → high Halstead operand count
    void draw_rect(int x, int y, int w, int h,
                   int r, int g, int b, int alpha,
                   bool filled, bool antialiased) {
        if (w <= 0 || h <= 0) return;          // +1 +1
        if (alpha <= 0) return;                // +1

        if (filled) {                          // +1
            for (int row = y; row < y + h; ++row) {   // +1
                for (int col = x; col < x + w; ++col) { // +1
                    if (antialiased && (row == y || row == y + h - 1 ||
                                        col == x || col == x + w - 1)) { // +1 +1 +1
                        // blend pixel
                    }
                }
            }
        } else {
            // outline only — 4 edges
            for (int col = x; col < x + w; ++col) { // +1
                // top & bottom edge
            }
            for (int row = y; row < y + h; ++row) { // +1
                // left & right edge
            }
        }
    }
};

// ═══════════════════════════════════════════════════════════════════
// SOLID PRINCIPLE TEST CASES
// ═══════════════════════════════════════════════════════════════════

// ── [S] SRP VIOLATION ───────────────────────────────────────────────
// One class handles UI, persistence, networking AND business logic.
class GodObject {
public:
    // UI group
    void render();
    void handle_click(int x, int y);
    void draw_tooltip(const std::string& msg);
    // Persistence group
    void save_to_db();
    void load_from_db();
    void migrate_schema();
    // Networking group
    void send_request(const std::string& url);
    void parse_response(const std::string& json);
    void retry_on_failure();
    // Business logic group
    void calculate_discount(double price);
    void apply_tax(double rate);
private:
    int   ui_state_;
    int   db_conn_;
    int   net_socket_;
    int   biz_context_;
};

// ── [O] OCP VIOLATION ───────────────────────────────────────────────
// Adding a new shape requires modifying this function.
double compute_area(int shape_type, double a, double b) {
    switch (shape_type) {
        case 0: return a * a;               // square
        case 1: return a * b;               // rectangle
        case 2: return 3.14159 * a * a;     // circle
        case 3: return 0.5 * a * b;         // triangle
        case 4: return 2 * (a + b) * b;     // trapezoid
        case 5: return a * b * 0.5 * 1.732; // hexagon approx
        default: return 0.0;
    }
}

// ── [L] LSP VIOLATION ───────────────────────────────────────────────
// Base defines a contract; derived breaks it by always throwing.
class FileStorage {
public:
    virtual std::string read(const std::string& path) { return ""; }
    virtual void write(const std::string& path, const std::string& data) {}
    virtual ~FileStorage() = default;
};

class ReadOnlyStorage : public FileStorage {
public:
    // Violates LSP: callers of FileStorage::write expect it to succeed.
    void write(const std::string& path, const std::string& data) override {
        throw std::runtime_error("ReadOnlyStorage: writes not supported");
    }
};

// ── [I] ISP VIOLATION ───────────────────────────────────────────────
// A fat interface forces every implementor to stub methods it doesn't need.
class IFatDevice {
public:
    virtual void print()    = 0;
    virtual void scan()     = 0;
    virtual void fax()      = 0;
    virtual void copy()     = 0;
    virtual void staple()   = 0;
    virtual void email()    = 0;
    virtual void ocr()      = 0;
    virtual void shred()    = 0;
    virtual ~IFatDevice()   = default;
};

// ── [D] DIP VIOLATION ───────────────────────────────────────────────
// High-level module hard-codes concrete low-level dependencies.
class ReportService {
public:
    void generate_report(const std::string& data) {
        // Creates three concrete objects — hard to test/swap.
        auto* logger   = new FileLogger();
        auto* exporter = new PdfExporter();
        auto* mailer   = new SmtpMailer();
        logger->log("generating report");
        exporter->export_pdf(data);
        mailer->send("admin@example.com", data);
        delete logger;
        delete exporter;
        delete mailer;
    }

private:
    struct FileLogger  { void log(const std::string&) {} };
    struct PdfExporter { void export_pdf(const std::string&) {} };
    struct SmtpMailer  { void send(const std::string&, const std::string&) {} };
};

// ── CLEAN COUNTER-EXAMPLES ──────────────────────────────────────────

// Good O: each shape is an extension, not a modification.
struct Shape { virtual double area() const = 0; virtual ~Shape() = default; };
struct Square   : Shape { double side; double area() const override { return side * side; } };
struct Circle   : Shape { double r;    double area() const override { return 3.14159 * r * r; } };

// Good D: dependencies are injected.
struct ILogger   { virtual void log(const std::string&) = 0; virtual ~ILogger() = default; };
struct IExporter { virtual void export_data(const std::string&) = 0; virtual ~IExporter() = default; };

class CleanReportService {
    ILogger*   logger_;
    IExporter* exporter_;
public:
    CleanReportService(ILogger* l, IExporter* e) : logger_(l), exporter_(e) {}
    void generate(const std::string& data) {
        logger_->log("generating");
        exporter_->export_data(data);
    }
};