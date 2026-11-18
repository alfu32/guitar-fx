# Arpeggio: play single notes; each one climbs a minor-7 chord in eighth notes, with echo and room.
compressor ratio=3,threshold=-20
arpeggio chord=minor7,pattern=bounce,rate=1/8,bpm=110,gate=0.55,mix=0.75
echo time=410,feedback=0.35,mix=0.25
reverb mix=0.3,time=3
