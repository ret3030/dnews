import sys
from urllib.parse import urlparse
import hashlib
from datetime import datetime

RESET = '\033[0m'
DIM   = '\033[38;5;242m'

LABELS = {
    'theguardian.com':         'GRD',
    'irozhlas.cz':             'IRO',
    'noahpinion.blog':         'NOA',
    'fredblog.stlouisfed.org': 'FED',
    'cnb.cz':                  'CNB',
    'bis.org':                 'BIS',
}

def domain(u):
    try: return urlparse(u).netloc.replace('www.', '')
    except: return ''

def color_for(d):
    palette = [166, 108, 132, 72, 172, 167, 66, 142]
    idx = int(hashlib.md5(d.encode()).hexdigest(), 16) % len(palette)
    return '\033[38;5;%dm' % palette[idx]

for line in sys.stdin:
    parts = line.rstrip().split('\x01')
    if len(parts) < 3: continue
    title, pubdate, url = parts[0], parts[1], parts[2]
    d = domain(url)
    label = LABELS.get(d, d.split('.')[0].upper()[:3])
    color = color_for(d)
    try: t = datetime.fromtimestamp(int(pubdate)).strftime('%H:%M')
    except: t = ''
    print('%s%s%s  %s  %s%s%s\x01%s\x01%s' % (color, label, RESET, title, DIM, t, RESET, pubdate, url))
