/* Thin C wrapper over libvpx so Rust never mirrors libvpx's struct layouts. */
#include <stdlib.h>
#include <string.h>
#include <vpx/vpx_decoder.h>
#include <vpx/vp8dx.h>

typedef struct {
    vpx_codec_ctx_t ctx;
    vpx_codec_iter_t iter;
} rd_vpx;

typedef struct {
    const unsigned char *planes[3];
    int strides[3];
    int width;
    int height;
    int fmt; /* 0 = I420, 1 = I444, -1 = other/unsupported */
} rd_vpx_image;

void *rd_vpx_open(int vp9, int threads) {
    rd_vpx *d = calloc(1, sizeof(rd_vpx));
    if (!d) return NULL;
    vpx_codec_dec_cfg_t cfg;
    memset(&cfg, 0, sizeof(cfg));
    cfg.threads = threads > 0 ? (unsigned)threads : 1;
    vpx_codec_iface_t *iface = vp9 ? vpx_codec_vp9_dx() : vpx_codec_vp8_dx();
    if (vpx_codec_dec_init(&d->ctx, iface, &cfg, 0) != VPX_CODEC_OK) {
        free(d);
        return NULL;
    }
    return d;
}

int rd_vpx_decode(void *h, const unsigned char *data, unsigned int len) {
    rd_vpx *d = h;
    d->iter = NULL;
    return (int)vpx_codec_decode(&d->ctx, data, len, NULL, 0);
}

const char *rd_vpx_error(void *h) {
    rd_vpx *d = h;
    return vpx_codec_error(&d->ctx);
}

int rd_vpx_next(void *h, rd_vpx_image *out) {
    rd_vpx *d = h;
    vpx_image_t *img = vpx_codec_get_frame(&d->ctx, &d->iter);
    if (!img) return 0;
    for (int i = 0; i < 3; i++) {
        out->planes[i] = img->planes[i];
        out->strides[i] = img->stride[i];
    }
    out->width = (int)img->d_w;
    out->height = (int)img->d_h;
    if (img->fmt == VPX_IMG_FMT_I420) out->fmt = 0;
    else if (img->fmt == VPX_IMG_FMT_I444) out->fmt = 1;
    else out->fmt = -1;
    return 1;
}

void rd_vpx_close(void *h) {
    rd_vpx *d = h;
    if (!d) return;
    vpx_codec_destroy(&d->ctx);
    free(d);
}
