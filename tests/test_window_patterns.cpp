#include "../plugins/qml/WindowPattern.hpp"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <cstdio>

#ifdef WW_TEST_GLIB
#    include <glib.h>
#endif

int main(int argc, char** argv) {
    if (argc != 2) return 1;
    QFile input(QString::fromLocal8Bit(argv[1]));
    if (! input.open(QIODevice::ReadOnly)) return 1;
    const auto document = QJsonDocument::fromJson(input.readAll());
    if (! document.isArray() || document.array().isEmpty()) return 1;
    int index = 0;
    for (const auto& entry : document.array()) {
        const auto test     = entry.toObject();
        const auto pattern  = test.value("pattern").toString();
        const auto value    = test.value("value").toString();
        const bool expected = test.value("matched").toBool();
        if (WindowPattern(pattern).matches(value) != expected) {
            std::fprintf(stderr, "Qt pattern vector %d failed\n", index);
            return 1;
        }
#ifdef WW_TEST_GLIB
        auto*      compiled = g_pattern_spec_new(pattern.toUtf8().constData());
        const bool matched  = g_pattern_spec_match_string(compiled, value.toUtf8().constData());
        g_pattern_spec_free(compiled);
        if (matched != expected) {
            std::fprintf(stderr, "GLib pattern vector %d failed\n", index);
            return 1;
        }
#endif
        ++index;
    }
    std::printf("%d pattern vectors passed\n", index);
    return 0;
}
