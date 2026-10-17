/*
 * Minimal C ABI over LG's StarfishMediaAPIs (libplayerAPIs), the media pipeline Cobalt
 * (YouTube), webOS Moonlight (ss4s) and plx-native use. Unlike NDL DirectMedia it takes the
 * exported punch-through window id in the Load payload (option.windowId), which lets the
 * compositor place and scale the decoded picture (SDL_webOSSetExportedWindow).
 *
 * Everything is resolved at runtime with dlopen/dlsym, so the app still starts where the library
 * is missing. The C++ ABI details (mangled names, in-place construction into an over-allocated
 * buffer, Feed's hidden std::string return slot, the pre-C++11 COW string fallback) follow
 * plx-native's src/starfish.c (GPL-3.0) and ss4s modules/webos/smp (LGPL-3.0); see docs/SOURCES.md.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define SMP_OBJECT_SIZE 65536u

typedef void (*smp_cb_fn)(int type, long long num, const char *str, void *ctx);
typedef void (*ctor_fn)(void *self, const char *uid);
typedef void (*dtor_fn)(void *self);
typedef int (*load_ctx_fn)(void *self, const char *payload, smp_cb_fn cb, void *ctx);
typedef int (*load_fn)(void *self, const char *payload, void (*cb)(int, long long, const char *));
typedef void (*feed_fn)(void *sret, void *self, const char *payload);
typedef void (*cow_dtor_fn)(void *str);
typedef int (*verb_fn)(void *self);
typedef void (*notify_fn)(void *self);

static struct {
    int resolved;
    char error[160];
    ctor_fn ctor;
    dtor_fn dtor;
    load_ctx_fn load_ctx;
    load_fn load;
    feed_fn feed;
    cow_dtor_fn cow_dtor;
    verb_fn play;
    verb_fn unload;
    verb_fn push_eos;
    notify_fn notify_foreground;
} S;

struct rdtv_smp {
    volatile int events[64];
    volatile int load_completed;
    volatile int error;
    volatile int playing;
    unsigned char object[SMP_OBJECT_SIZE] __attribute__((aligned(16)));
};

static void *sym(void *h, const char *name) {
    void *p = dlsym(h, name);
    return p ? p : dlsym(RTLD_DEFAULT, name);
}

/* Returns NULL when usable, else a static description of what is missing. */
const char *rdtv_smp_available(void) {
    if (S.resolved) return S.error[0] ? S.error : NULL;
    S.resolved = 1;
    void *h = dlopen("libplayerAPIs.so.1", RTLD_NOW | RTLD_GLOBAL);
    if (!h) h = dlopen("libplayerAPIs.so", RTLD_NOW | RTLD_GLOBAL);
    if (!h) {
        snprintf(S.error, sizeof S.error, "libplayerAPIs not found: %s", dlerror());
        return S.error;
    }
    S.ctor = (ctor_fn)sym(h, "_ZN17StarfishMediaAPIsC1EPKc");
    S.dtor = (dtor_fn)sym(h, "_ZN17StarfishMediaAPIsD1Ev");
    S.load_ctx = (load_ctx_fn)sym(h, "_ZN17StarfishMediaAPIs4LoadEPKcPFvixS1_PvES2_");
    S.load = (load_fn)sym(h, "_ZN17StarfishMediaAPIs4LoadEPKcPFvixS1_E");
    S.feed = (feed_fn)sym(h, "_ZN17StarfishMediaAPIs4FeedB5cxx11EPKc");
    if (!S.feed) {
        S.feed = (feed_fn)sym(h, "_ZN17StarfishMediaAPIs4FeedEPKc");
        if (S.feed) S.cow_dtor = (cow_dtor_fn)sym(h, "_ZNSsD1Ev");
    }
    S.play = (verb_fn)sym(h, "_ZN17StarfishMediaAPIs4PlayEv");
    S.unload = (verb_fn)sym(h, "_ZN17StarfishMediaAPIs6UnloadEv");
    S.push_eos = (verb_fn)sym(h, "_ZN17StarfishMediaAPIs7pushEOSEv");
    S.notify_foreground = (notify_fn)sym(h, "_ZN17StarfishMediaAPIs16notifyForegroundEv");
    if (!S.ctor || !S.dtor || !S.load_ctx || !S.feed || !S.play || !S.unload) {
        snprintf(S.error, sizeof S.error,
                 "StarfishMediaAPIs symbols missing (ctor=%d load=%d feed=%d play=%d unload=%d)",
                 !!S.ctor, !!S.load_ctx, !!S.feed, !!S.play, !!S.unload);
        return S.error;
    }
    return NULL;
}

#define EV_LOADCOMPLETED 0x16
#define EV_PLAYING 0x1a
#define EV_INT_ERROR 0x12
#define EV_STR_ERROR 0x13

static void on_event(int type, long long num, const char *str, void *ctx) {
    struct rdtv_smp *p = ctx;
    if (!p) return;
    if (type >= 0 && type < 64) p->events[type]++;
    switch (type) {
        case EV_LOADCOMPLETED:
            p->load_completed = 1;
            S.play(p->object);
            break;
        case EV_PLAYING:
            p->playing = 1;
            break;
        case EV_INT_ERROR:
        case EV_STR_ERROR:
            fprintf(stderr, "rdtv smp: error event 0x%02x num=%lld %s\n", type, num, str ? str : "");
            p->error = 1;
            break;
        default:
            break;
    }
}

/* Construct and Load. Returns NULL on failure. */
struct rdtv_smp *rdtv_smp_load(const char *payload) {
    if (rdtv_smp_available()) return NULL;
    struct rdtv_smp *p = NULL;
    if (posix_memalign((void **)&p, 16, sizeof *p) != 0 || !p) return NULL;
    memset(p, 0, sizeof *p);
    S.ctor(p->object, NULL);
    if (S.notify_foreground) S.notify_foreground(p->object);
    int ok = S.load_ctx(p->object, payload, on_event, p);
    if (!(ok & 0xff)) {
        /* The object may still reach the library's threads: never free it. */
        fprintf(stderr, "rdtv smp: Load refused\n");
        return NULL;
    }
    return p;
}

int rdtv_smp_load_completed(const struct rdtv_smp *p) { return p->load_completed; }
int rdtv_smp_failed(const struct rdtv_smp *p) { return p->error; }

/* Feed one access unit. 'O' accepted, 'B' buffer full, 'e' error. */
char rdtv_smp_feed(struct rdtv_smp *p, const unsigned char *data, unsigned size,
                   unsigned long long pts_ns) {
    char j[160];
    snprintf(j, sizeof j, "{\"bufferAddr\":\"%p\",\"bufferSize\":%u,\"pts\":%llu,\"esData\":1}",
             (const void *)data, size, pts_ns);
    unsigned char ret[32];
    memset(ret, 0, sizeof ret);
    S.feed(ret, p->object, j);
    const char *s = *(char **)ret;
    char verdict = !s ? 'e' : strstr(s, "BufferFull") ? 'B' : strstr(s, "Ok") ? 'O' : 'e';
    if (s && S.cow_dtor) S.cow_dtor(ret);
    return verdict;
}

/* Unload and destroy. The storage is deliberately leaked: a late library callback must never
 * land in freed memory (one 64 KiB block per decoder reload). */
void rdtv_smp_unload(struct rdtv_smp *p) {
    if (!p) return;
    if (S.push_eos) S.push_eos(p->object);
    S.unload(p->object);
    S.dtor(p->object);
}
