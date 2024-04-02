import sofar as sf
import sofa

path = '/Users/Owen/Documents/GitHub/ase-project/SOFA-data/HRIR_FULL2DEG.sofa'
sofa.Database.open(path).Metadata.dump()

sofar_data = sf.read_sofa(path)
sofar_data.inspect()