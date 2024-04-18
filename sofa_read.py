import sofar as sf

def read_sofa(path):
    sofa_read = sf.read_sofa(path, verify=False)

    # inspect = sofa_read.inspect()
    ir = sofa_read.Data_IR                  # 3D shape: (M: measurements, R: receivers, N: samples)
    source_pos = sofa_read.SourcePosition   # 2D shape: (M: measurements, C: Number of coordinates (always 3))
    rec_pos = sofa_read.ReceiverPosition

    # count = 0
    # for p in source_pos:
    #     if count % 1 == 0:
    #         print(p)
    #     count += 1 

    print(ir.shape)
    print(source_pos.shape)
    print(rec_pos.shape)       

if __name__ == "__main__":
    path_spherical = '/Users/Owen/Documents/GitHub/ase-project/SOFA-data/HRIR_FULL2DEG.sofa'
    # HRIR_FULL2DEG: 
    #   sample number: 128
    #   two degree azimuth change from 0 deg to 358 deg; 
    #   two degree elevation change from 90 deg to -90 deg; 
    #   The distance is alwasy 3.25

    path_cartesian = '/Users/Owen/Documents/GitHub/ase-project/SOFA-data/BRIR_Audimax_LSC_KU100_P2_circ360.sofa'

    read_sofa(path_spherical)
    # read_sofa(path_cartesian)


