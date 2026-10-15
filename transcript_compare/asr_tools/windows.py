"""Print what each transcript says in given time windows. usage: windows.py <meeting_dir> <a-b> [<a-b> ...] (seconds)"""
import json,sys,os
TC=sys.argv[1]  # the meeting folder
def mt(f):
    return [(s['audio_start_time'],s['audio_end_time'],s['source_device'],s['text']) for s in json.load(open(os.path.join(TC,f),encoding='utf-8'))['segments']]
def at(f):
    return [(s['start'],s['end'],s['source'],s['text']) for s in json.load(open(os.path.join(TC,'asr_runs',f),encoding='utf-8'))['segments']]
src={}
for n,f in (('online','transcripts_online.json'),('offline','transcripts_offline.json'),('meetily','transcripts.json')):
    if os.path.exists(os.path.join(TC,f)): src[n]=mt(f)
for n,f in (('fw-v3','asr_fw-large-v3.json'),('fw-turbo','asr_fw-large-v3-turbo.json'),('gigaam','asr_gigaam-v3-e2e-rnnt.json'),('parakeet','asr_nemo-parakeet-tdt-0.6b-v3.json')):
    if os.path.exists(os.path.join(TC,'asr_runs',f)): src[n]=at(f)
for w in sys.argv[2:]:
    a,b=map(float,w.split('-'))
    print(f'######## {int(a//60)}:{int(a%60):02d}-{int(b//60)}:{int(b%60):02d}')
    for n,s in src.items():
        t=' '.join(x[3] for x in sorted(s) if x[1]>a and x[0]<b)
        print(f'[{n}] {t}')
